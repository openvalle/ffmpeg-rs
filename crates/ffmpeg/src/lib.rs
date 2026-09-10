//! Runtime-loaded FFmpeg 7, 8 and 9, with one process-wide version selection.
#![allow(
    non_camel_case_types,
    clippy::missing_safety_doc,
    clippy::module_inception,
    clippy::too_many_arguments
)]
#[macro_use]
extern crate bitflags;

pub use sys::runtime::{LibraryInfo, LoadError, Runtime, Version, load, set_directory};
pub use valle_ffmpeg_sys as sys;

/// Version-specific APIs for integrations that need native pointers. Raw objects must stay within
/// their selected backend. Prefer the version-neutral types at the crate root for normal use.
pub mod backend {
    pub mod v7 {
        include!(concat!(env!("OUT_DIR"), "/v7/lib.rs"));
    }
    pub mod v8 {
        include!(concat!(env!("OUT_DIR"), "/v8/lib.rs"));
    }
    pub mod v9 {
        include!(concat!(env!("OUT_DIR"), "/v9/lib.rs"));
    }
}

pub fn init() -> std::result::Result<&'static Runtime, LoadError> {
    let runtime = load()?;
    match runtime.version {
        Version::V7 => backend::v7::init().map_err(|e| LoadError(e.to_string()))?,
        Version::V8 => backend::v8::init().map_err(|e| LoadError(e.to_string()))?,
        Version::V9 => backend::v9::init().map_err(|e| LoadError(e.to_string()))?,
    }
    set_log_level(LOG_LEVEL.load(std::sync::atomic::Ordering::Relaxed));
    Ok(runtime)
}

/// An operation error, with the native FFmpeg error code when available.
#[derive(Debug, Clone)]
pub struct Error {
    pub code: Option<i32>,
    message: String,
}
impl Error {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<LoadError> for Error {
    fn from(error: LoadError) -> Self {
        Self::message(error.to_string())
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Registered codec information. Registration does not imply a hardware device is available.
#[derive(Debug, Clone)]
pub struct CodecInfo {
    pub name: String,
    pub description: String,
    pub encoder: bool,
    pub decoder: bool,
}
impl From<backend::v7::Error> for Error {
    fn from(error: backend::v7::Error) -> Self {
        Self {
            code: Some(error.into()),
            message: error.to_string(),
        }
    }
}
impl From<backend::v8::Error> for Error {
    fn from(error: backend::v8::Error) -> Self {
        Self {
            code: Some(error.into()),
            message: error.to_string(),
        }
    }
}
impl From<backend::v9::Error> for Error {
    fn from(error: backend::v9::Error) -> Self {
        Self {
            code: Some(error.into()),
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "codec")]
pub fn codecs() -> Result<Vec<CodecInfo>> {
    let runtime = init()?;
    macro_rules! list {
        ($ff:ident) => {{
            let mut result = Vec::new();
            let mut state = std::ptr::null_mut();
            unsafe {
                loop {
                    let codec = $ff::ffi::av_codec_iterate(&mut state);
                    if codec.is_null() {
                        break;
                    }
                    let text = |p: *const libc::c_char| {
                        if p.is_null() {
                            String::new()
                        } else {
                            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                        }
                    };
                    result.push(CodecInfo {
                        name: text((*codec).name),
                        description: text((*codec).long_name),
                        encoder: $ff::ffi::av_codec_is_encoder(codec) != 0,
                        decoder: $ff::ffi::av_codec_is_decoder(codec) != 0,
                    });
                }
            }
            result.sort_by(|a, b| a.name.cmp(&b.name));
            result
        }};
    }
    use backend::{v7, v8, v9};
    Ok(match runtime.version {
        Version::V7 => list!(v7),
        Version::V8 => list!(v8),
        Version::V9 => list!(v9),
    })
}

#[cfg(all(feature = "format", feature = "software-scaling"))]
mod video;
#[cfg(all(feature = "format", feature = "software-scaling"))]
pub use video::{VideoDecoder, VideoEncoder, VideoEncoderOptions, VideoFrame};

static LOG_LEVEL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(32);
/// Set the FFmpeg log level, without loading an absent runtime. Applied on subsequent initialization.
pub fn set_log_level(level: i32) {
    LOG_LEVEL.store(level, std::sync::atomic::Ordering::Relaxed);
    if let Some(runtime) = sys::runtime::current() {
        unsafe {
            match runtime.version {
                Version::V7 => sys::abi7::av_log_set_level(level),
                Version::V8 => sys::abi8::av_log_set_level(level),
                Version::V9 => sys::abi9::av_log_set_level(level),
            }
        }
    }
}
