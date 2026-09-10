//! Runtime loading keeps libav out of the executable's startup dependencies.
//! Libraries are validated as a complete set before calling any layout-dependent API.
use libloading::Library;
use std::{
    ffi::OsStr,
    fmt,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[derive(Debug, Clone)]
pub struct LoadError(pub String);
impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for LoadError {}

#[derive(Debug, Clone)]
pub struct LibraryInfo {
    pub name: &'static str,
    pub path: PathBuf,
    pub version: u32,
}

pub struct Runtime {
    // Keep every library alive until process exit, including during frame/codec Drop.
    _libraries: Libraries,
    pub(crate) functions: Functions,
    pub version: Version,
    pub libraries: Vec<LibraryInfo>,
}
pub(crate) struct Libraries(Vec<Library>);
impl Libraries {
    pub(crate) unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, LoadError> {
        for library in &self.0 {
            if let Ok(symbol) = unsafe { library.get::<T>(name) } {
                return Ok(*symbol);
            }
        }
        Err(LoadError(format!(
            "FFmpeg is missing required symbol {}; install a complete compatible shared-library build",
            String::from_utf8_lossy(name).trim_end_matches('\0')
        )))
    }
}

/// FFmpeg release family; distinct from the Rust package version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V7,
    V8,
    V9,
}
impl Version {
    pub const fn major(self) -> u32 {
        match self {
            Self::V7 => 7,
            Self::V8 => 8,
            Self::V9 => 9,
        }
    }
}
pub(crate) enum Functions {
    V7(crate::abi7::DynamicFunctions),
    V8(crate::abi8::DynamicFunctions),
    V9(crate::abi9::DynamicFunctions),
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static DIRECTORY: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Set a runtime library directory or installation prefix before the first media operation.
/// An explicit path is authoritative: do not silently fall back to a different installation.
pub fn set_directory(directory: PathBuf) -> Result<(), LoadError> {
    let mut configured = DIRECTORY.lock().unwrap_or_else(|e| e.into_inner());
    if RUNTIME.get().is_some() {
        return Err(LoadError(
            "FFmpeg is already loaded; restart before changing its directory".into(),
        ));
    }
    *configured = Some(directory);
    Ok(())
}

/// Load once on success. A missing installation can be installed and retried in a long-lived host.
pub fn load() -> Result<&'static Runtime, LoadError> {
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let configured = DIRECTORY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let explicit = configured
        .clone()
        .or_else(|| std::env::var_os("VALLE_FFMPEG_DIR").map(PathBuf::from));
    let candidates = search_directories(explicit.as_deref());
    let mut errors = Vec::new();
    for candidate in candidates {
        for version in [Version::V9, Version::V8, Version::V7] {
            match load_directory(candidate.as_deref(), version) {
                Ok(runtime) => {
                    let _ = RUNTIME.set(runtime);
                    return Ok(RUNTIME.get().unwrap());
                }
                Err(e) => errors.push(format!("FFmpeg {}: {e}", version.major())),
            }
        }
    }
    Err(LoadError(format!(
        "FFmpeg shared libraries are unavailable or incompatible for {}-{}. Install FFmpeg 7, 8 or 9 shared libraries (including their dependencies), then set VALLE_FFMPEG_DIR to the installation/lib directory. A standalone ffmpeg executable is insufficient.\n{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        errors.join("\n")
    )))
}

/// Return the current runtime without attempting to load FFmpeg.
pub fn current() -> Option<&'static Runtime> {
    RUNTIME.get()
}

pub(crate) fn loaded() -> &'static Runtime {
    // Safe wrapper constructors in external consumers may call FFI without Valle's initialization.
    // Production Valle entry points return load() errors before invoking those constructors.
    RUNTIME
        .get()
        .unwrap_or_else(|| load().unwrap_or_else(|e| panic!("{e}")))
}

fn search_directories(explicit: Option<&Path>) -> Vec<Option<PathBuf>> {
    if let Some(path) = explicit {
        return vec![
            Some(path.to_owned()),
            Some(path.join("lib")),
            Some(path.join("bin")),
        ];
    }
    let mut paths = Vec::new();
    #[cfg(target_os = "macos")]
    for path in [
        "/opt/homebrew/opt/ffmpeg/lib",
        "/opt/homebrew/opt/ffmpeg@8/lib",
        "/opt/homebrew/opt/ffmpeg@7/lib",
        "/usr/local/opt/ffmpeg/lib",
        "/usr/local/opt/ffmpeg@8/lib",
        "/usr/local/opt/ffmpeg@7/lib",
        "/opt/local/lib",
        "/usr/local/lib",
    ] {
        paths.push(Some(PathBuf::from(path)));
    }
    #[cfg(target_os = "linux")]
    {
        for path in ["/usr/local/lib", "/usr/lib", "/usr/lib64"] {
            paths.push(Some(PathBuf::from(path)));
        }
        let triple = match std::env::consts::ARCH {
            "x86_64" => "x86_64-linux-gnu",
            "aarch64" => "aarch64-linux-gnu",
            _ => "",
        };
        if !triple.is_empty() {
            paths.push(Some(PathBuf::from(format!("/usr/lib/{triple}"))));
        }
        // ld.so resolves versioned library names using the system cache and configured search path.
        paths.push(None);
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(
            std::env::split_paths(&path)
                .filter(|p| p.is_absolute())
                .map(Some),
        );
    }
    paths
}

fn specifications(version: Version) -> Vec<(&'static str, u32)> {
    match version {
        Version::V7 => vec![
            (
                "avutil",
                ((crate::abi7::LIBAVUTIL_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBAVUTIL_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBAVUTIL_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "swresample")]
            (
                "swresample",
                ((crate::abi7::LIBSWRESAMPLE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBSWRESAMPLE_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBSWRESAMPLE_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "swscale")]
            (
                "swscale",
                ((crate::abi7::LIBSWSCALE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBSWSCALE_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBSWSCALE_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avcodec")]
            (
                "avcodec",
                ((crate::abi7::LIBAVCODEC_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBAVCODEC_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBAVCODEC_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avformat")]
            (
                "avformat",
                ((crate::abi7::LIBAVFORMAT_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBAVFORMAT_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBAVFORMAT_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avfilter")]
            (
                "avfilter",
                ((crate::abi7::LIBAVFILTER_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBAVFILTER_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBAVFILTER_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avdevice")]
            (
                "avdevice",
                ((crate::abi7::LIBAVDEVICE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi7::LIBAVDEVICE_VERSION_MINOR as u32) << 8)
                    | crate::abi7::LIBAVDEVICE_VERSION_MICRO as u32,
            ),
        ],
        Version::V8 => vec![
            (
                "avutil",
                ((crate::abi8::LIBAVUTIL_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBAVUTIL_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBAVUTIL_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "swresample")]
            (
                "swresample",
                ((crate::abi8::LIBSWRESAMPLE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBSWRESAMPLE_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBSWRESAMPLE_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "swscale")]
            (
                "swscale",
                ((crate::abi8::LIBSWSCALE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBSWSCALE_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBSWSCALE_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avcodec")]
            (
                "avcodec",
                ((crate::abi8::LIBAVCODEC_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBAVCODEC_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBAVCODEC_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avformat")]
            (
                "avformat",
                ((crate::abi8::LIBAVFORMAT_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBAVFORMAT_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBAVFORMAT_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avfilter")]
            (
                "avfilter",
                ((crate::abi8::LIBAVFILTER_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBAVFILTER_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBAVFILTER_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avdevice")]
            (
                "avdevice",
                ((crate::abi8::LIBAVDEVICE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi8::LIBAVDEVICE_VERSION_MINOR as u32) << 8)
                    | crate::abi8::LIBAVDEVICE_VERSION_MICRO as u32,
            ),
        ],
        Version::V9 => vec![
            (
                "avutil",
                ((crate::abi9::LIBAVUTIL_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBAVUTIL_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBAVUTIL_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "swresample")]
            (
                "swresample",
                ((crate::abi9::LIBSWRESAMPLE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBSWRESAMPLE_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBSWRESAMPLE_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "swscale")]
            (
                "swscale",
                ((crate::abi9::LIBSWSCALE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBSWSCALE_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBSWSCALE_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avcodec")]
            (
                "avcodec",
                ((crate::abi9::LIBAVCODEC_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBAVCODEC_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBAVCODEC_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avformat")]
            (
                "avformat",
                ((crate::abi9::LIBAVFORMAT_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBAVFORMAT_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBAVFORMAT_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avfilter")]
            (
                "avfilter",
                ((crate::abi9::LIBAVFILTER_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBAVFILTER_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBAVFILTER_VERSION_MICRO as u32,
            ),
            #[cfg(feature = "avdevice")]
            (
                "avdevice",
                ((crate::abi9::LIBAVDEVICE_VERSION_MAJOR as u32) << 16)
                    | ((crate::abi9::LIBAVDEVICE_VERSION_MINOR as u32) << 8)
                    | crate::abi9::LIBAVDEVICE_VERSION_MICRO as u32,
            ),
        ],
    }
}

fn library_name(name: &str, version: u32) -> String {
    let major = version >> 16;
    if cfg!(target_os = "macos") {
        format!("lib{name}.{major}.dylib")
    } else if cfg!(target_os = "windows") {
        format!("{name}-{major}.dll")
    } else {
        format!("lib{name}.so.{major}")
    }
}

fn version_text(version: u32) -> String {
    format!(
        "{}.{}.{}",
        version >> 16,
        (version >> 8) & 255,
        version & 255
    )
}

fn validate_version(name: &str, actual: u32, expected: u32) -> Result<(), LoadError> {
    if actual >> 16 != expected >> 16 || actual < expected {
        return Err(LoadError(format!(
            "{name} ABI/version mismatch: found {}, require >= {} within ABI major {}",
            version_text(actual),
            version_text(expected),
            expected >> 16
        )));
    }
    Ok(())
}

fn load_directory(directory: Option<&Path>, version: Version) -> Result<Runtime, LoadError> {
    let mut libraries = Libraries(Vec::new());
    let mut information = Vec::new();
    // Open and validate the entire set before loading wrappers that access C structs.
    for (name, expected) in specifications(version) {
        let filename = library_name(name, expected);
        let path = directory.map_or_else(|| PathBuf::from(&filename), |dir| dir.join(&filename));
        if directory.is_some() {
            check_architecture(&path).map_err(|e| LoadError(format!("{}: {e}", path.display())))?;
        }
        let path = if directory.is_some() {
            path.canonicalize().map_err(|e| LoadError(e.to_string()))?
        } else {
            path
        };
        let library = unsafe { open_library(path.as_os_str()) }.map_err(|e| {
            LoadError(format!(
                "cannot load {} for {}: {}",
                path.display(),
                std::env::consts::ARCH,
                describe_error(&e)
            ))
        })?;
        let symbol = format!("{name}_version\0");
        let version = unsafe {
            let function = library
                .get::<unsafe extern "C" fn() -> u32>(symbol.as_bytes())
                .map_err(|e| {
                    LoadError(format!("{}: missing {name}_version: {e}", path.display()))
                })?;
            function()
        };
        validate_version(name, version, expected)?;
        information.push(LibraryInfo {
            name,
            path,
            version,
        });
        libraries.0.push(library);
    }
    validate_dependencies(&libraries, &information)?;
    let functions = unsafe {
        match version {
            Version::V7 => Functions::V7(crate::abi7::DynamicFunctions::load(&libraries)?),
            Version::V8 => Functions::V8(crate::abi8::DynamicFunctions::load(&libraries)?),
            Version::V9 => Functions::V9(crate::abi9::DynamicFunctions::load(&libraries)?),
        }
    };
    Ok(Runtime {
        _libraries: libraries,
        functions,
        version,
        libraries: information,
    })
}

/// Reject a library whose transitive libav dependencies differ from the selected set. Matching
/// SONAMEs alone is insufficient: adjacent FFmpeg releases can share a swresample ABI major.
fn validate_dependencies(
    libraries: &Libraries,
    information: &[LibraryInfo],
) -> Result<(), LoadError> {
    #[cfg(not(target_os = "windows"))]
    for (library, source) in libraries.0.iter().zip(information) {
        for dependency in information {
            if source.name == dependency.name {
                continue;
            }
            let symbol = format!("{}_version\0", dependency.name);
            // POSIX dlsym on a handle searches that library's dependency scope. These version
            // functions take no arguments and do not access any caller-provided C layout.
            if let Ok(function) =
                unsafe { library.get::<unsafe extern "C" fn() -> u32>(symbol.as_bytes()) }
            {
                let actual = unsafe { function() };
                if actual != dependency.version {
                    return Err(LoadError(format!(
                        "mixed FFmpeg library set: {} resolves {} {}, but the selected set uses {} {}; provide one complete installation",
                        source.path.display(),
                        dependency.name,
                        version_text(actual),
                        dependency.name,
                        version_text(dependency.version)
                    )));
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    for source in information {
        // GetProcAddress does not search dependencies. Check their versioned DLL import names.
        let bytes = std::fs::read(&source.path).map_err(|e| LoadError(e.to_string()))?;
        for imported in pe_imports(&bytes)? {
            for dependency in information {
                if imported.starts_with(&format!("{}-", dependency.name))
                    && imported != library_name(dependency.name, dependency.version)
                {
                    return Err(LoadError(format!(
                        "mixed FFmpeg library set: {} imports {imported}, expected {}; provide one complete installation",
                        source.path.display(),
                        library_name(dependency.name, dependency.version)
                    )));
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    let _ = libraries;
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn pe_imports(bytes: &[u8]) -> Result<Vec<String>, LoadError> {
    let invalid = || LoadError("invalid PE import table".into());
    let u16_at = |offset: usize| -> Result<u16, LoadError> {
        Ok(u16::from_le_bytes(
            bytes
                .get(offset..offset.checked_add(2).ok_or_else(invalid)?)
                .ok_or_else(invalid)?
                .try_into()
                .unwrap(),
        ))
    };
    let u32_at = |offset: usize| -> Result<u32, LoadError> {
        Ok(u32::from_le_bytes(
            bytes
                .get(offset..offset.checked_add(4).ok_or_else(invalid)?)
                .ok_or_else(invalid)?
                .try_into()
                .unwrap(),
        ))
    };
    let pe = u32_at(60)? as usize;
    if bytes.get(pe..pe.checked_add(4).ok_or_else(invalid)?) != Some(b"PE\0\0") {
        return Err(invalid());
    }
    let section_count = u16_at(pe + 6)? as usize;
    if section_count > 96 {
        return Err(invalid());
    }
    let optional = pe + 24;
    let optional_size = u16_at(pe + 20)? as usize;
    let directories = match u16_at(optional)? {
        0x10b => 96,
        0x20b => 112,
        _ => return Err(invalid()),
    };
    if optional_size < directories + 16 {
        return Err(invalid());
    }
    let import_rva = u32_at(optional + directories + 8)?;
    if import_rva == 0 {
        return Ok(Vec::new());
    }
    let section_table = optional + optional_size;
    let file_offset = |rva: u32| -> Result<usize, LoadError> {
        for index in 0..section_count {
            let section = section_table + index * 40;
            let virtual_address = u32_at(section + 12)?;
            let raw_size = u32_at(section + 16)?;
            let raw_pointer = u32_at(section + 20)?;
            if let Some(delta) = rva.checked_sub(virtual_address)
                && delta < raw_size
            {
                let offset = raw_pointer.checked_add(delta).ok_or_else(invalid)? as usize;
                if offset < bytes.len() {
                    return Ok(offset);
                }
            }
        }
        Err(invalid())
    };
    let mut names = Vec::new();
    for index in 0..1024u32 {
        let rva = import_rva.checked_add(index * 20).ok_or_else(invalid)?;
        let descriptor = file_offset(rva)?;
        let data = bytes.get(descriptor..descriptor + 20).ok_or_else(invalid)?;
        if data.iter().all(|byte| *byte == 0) {
            return Ok(names);
        }
        let name = file_offset(u32_at(descriptor + 12)?)?;
        let tail = bytes.get(name..).ok_or_else(invalid)?;
        let length = tail
            .iter()
            .take(260)
            .position(|byte| *byte == 0)
            .ok_or_else(invalid)?;
        names.push(String::from_utf8_lossy(&tail[..length]).to_ascii_lowercase());
    }
    Err(invalid())
}

fn describe_error(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        message.push_str(": ");
        message.push_str(&error.to_string());
        cause = error.source();
    }
    message
}

unsafe fn open_library(path: &OsStr) -> Result<Library, libloading::Error> {
    #[cfg(target_os = "windows")]
    {
        // Resolve dependencies alongside the selected DLL and in trusted system directories.
        // Do not search the current working directory.
        const SEARCH_DLL_LOAD_DIR: u32 = 0x100;
        const SEARCH_DEFAULT_DIRS: u32 = 0x1000;
        unsafe {
            libloading::os::windows::Library::load_with_flags(
                path,
                SEARCH_DLL_LOAD_DIR | SEARCH_DEFAULT_DIRS,
            )
            .map(Into::into)
        }
    }
    #[cfg(not(target_os = "windows"))]
    unsafe {
        Library::new(path)
    }
}

/// Inspect Mach-O (including universal), ELF and PE headers before executing library initializers.
fn check_architecture(path: &Path) -> Result<(), LoadError> {
    let mut file = File::open(path).map_err(|e| LoadError(e.to_string()))?;
    let mut head = [0u8; 64];
    file.read_exact(&mut head)
        .map_err(|e| LoadError(format!("invalid shared-library header: {e}")))?;
    let mut architectures = Vec::new();
    let cpu = |v: u32| match v {
        0x0100000c => "aarch64",
        0x01000007 => "x86_64",
        12 => "arm",
        7 => "x86",
        _ => "unknown",
    };
    match &head[..4] {
        [0xcf, 0xfa, 0xed, 0xfe] | [0xce, 0xfa, 0xed, 0xfe] => {
            architectures.push(cpu(u32::from_le_bytes(head[4..8].try_into().unwrap())));
        }
        [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf] => {
            let count = u32::from_be_bytes(head[4..8].try_into().unwrap());
            if count == 0 || count > 64 {
                return Err(LoadError("invalid universal Mach-O header".into()));
            }
            let stride = if head[3] == 0xbf { 32 } else { 20 };
            for index in 0..count {
                file.seek(SeekFrom::Start(8 + u64::from(index) * stride))
                    .map_err(|e| LoadError(e.to_string()))?;
                let mut value = [0; 4];
                file.read_exact(&mut value)
                    .map_err(|e| LoadError(e.to_string()))?;
                architectures.push(cpu(u32::from_be_bytes(value)));
            }
        }
        [0x7f, b'E', b'L', b'F'] => {
            let value = match head[5] {
                1 => u16::from_le_bytes(head[18..20].try_into().unwrap()),
                2 => u16::from_be_bytes(head[18..20].try_into().unwrap()),
                _ => return Err(LoadError("invalid ELF byte order".into())),
            };
            architectures.push(match value {
                183 => "aarch64",
                62 => "x86_64",
                40 => "arm",
                3 => "x86",
                _ => "unknown",
            });
        }
        _ if &head[..2] == b"MZ" => {
            let offset = u32::from_le_bytes(head[60..64].try_into().unwrap());
            file.seek(SeekFrom::Start(u64::from(offset)))
                .map_err(|e| LoadError(e.to_string()))?;
            let mut pe = [0u8; 6];
            file.read_exact(&mut pe)
                .map_err(|e| LoadError(e.to_string()))?;
            if &pe[..4] != b"PE\0\0" {
                return Err(LoadError("invalid PE signature".into()));
            }
            architectures.push(match u16::from_le_bytes(pe[4..6].try_into().unwrap()) {
                0xaa64 => "aarch64",
                0x8664 => "x86_64",
                0x14c => "x86",
                _ => "unknown",
            });
        }
        _ => return Err(LoadError("not a supported shared-library binary".into())),
    }
    if !architectures.contains(&std::env::consts::ARCH) {
        return Err(LoadError(format!(
            "architecture mismatch: library contains {architectures:?}, Valle requires {}",
            std::env::consts::ARCH
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn library_error_preserves_the_os_cause() {
        let directory = tempfile::tempdir().unwrap();
        let error = unsafe {
            super::open_library(directory.path().join("missing-library.dll").as_os_str())
        }
        .err()
        .unwrap();
        let cause = std::error::Error::source(&error).expect("OS loader failure has a cause");
        let message = super::describe_error(&error);
        assert!(message.starts_with(&error.to_string()));
        assert!(message.contains(&cause.to_string()));
        assert_ne!(message, error.to_string());
    }

    use super::*;
    #[test]
    fn reads_versioned_pe_dependencies_and_rejects_truncated_tables() {
        let mut bytes = vec![0u8; 1024];
        fn put16(bytes: &mut [u8], offset: usize, value: u16) {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn put32(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        put32(&mut bytes, 60, 0x80);
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        put16(&mut bytes, 0x86, 1);
        put16(&mut bytes, 0x94, 0xf0);
        put16(&mut bytes, 0x98, 0x20b);
        put32(&mut bytes, 0x98 + 112 + 8, 0x1000);
        let section = 0x98 + 0xf0;
        put32(&mut bytes, section + 12, 0x1000);
        put32(&mut bytes, section + 16, 0x200);
        put32(&mut bytes, section + 20, 0x200);
        put32(&mut bytes, 0x200 + 12, 0x1060);
        bytes[0x260..0x26e].copy_from_slice(b"avutil-61.dll\0");
        assert_eq!(pe_imports(&bytes).unwrap(), vec!["avutil-61.dll"]);
        assert!(pe_imports(&bytes[..0x205]).is_err());
        put32(&mut bytes, 0x200 + 12, u32::MAX);
        assert!(pe_imports(&bytes).is_err());
    }

    #[test]
    fn rejects_wrong_abi_and_older_headers() {
        assert!(validate_version("avcodec", 62 << 16, (63 << 16) | 256).is_err());
        assert!(validate_version("avcodec", 63 << 16, (63 << 16) | 256).is_err());
        assert!(validate_version("avcodec", (63 << 16) | 512, (63 << 16) | 256).is_ok());
    }
    #[test]
    fn explicit_directory_never_falls_back_to_system() {
        let path = Path::new("/missing/ffmpeg");
        assert_eq!(
            search_directories(Some(path)),
            vec![
                Some(path.to_owned()),
                Some(path.join("lib")),
                Some(path.join("bin"))
            ]
        );
    }
}
