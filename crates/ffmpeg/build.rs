use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(valle_enabled)");
    println!("cargo:rustc-check-cfg=cfg(valle_disabled)");
    println!("cargo:rustc-cfg=valle_enabled");
    println!("cargo:rerun-if-changed=src/backend/shared");
    println!("cargo:rerun-if-changed=src/video_backend.rs");
    for major in [7, 8, 9] {
        let output = PathBuf::from(env::var("OUT_DIR").unwrap()).join(format!("v{major}"));
        copy(Path::new("src/backend/shared"), &output, major);
        let source = fs::read_to_string("src/video_backend.rs").unwrap();
        fs::write(
            output.join("video_backend.rs"),
            source.replace(
                "use super::ff;",
                &format!("use crate::backend::v{major} as ff;"),
            ),
        )
        .unwrap();
    }
}

// Specialize the upstream compatibility branches into separate module namespaces. All inputs are
// shipped in this crate; generation never downloads code or modifies the source tree.
fn copy(source: &Path, destination: &Path, major: u32) {
    fs::create_dir_all(destination).unwrap();
    let versions = regex::Regex::new(r#"feature\s*=\s*"ffmpeg_(\d+)_(\d+)""#).unwrap();
    let retired =
        regex::Regex::new(r#"feature\s*=\s*"(?:ff_api_[^"]+|ffmpeg4[123]?|rpi)""#).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let path = entry.unwrap().path();
        let output = destination.join(path.file_name().unwrap());
        if path.is_dir() {
            copy(&path, &output, major);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let text = versions.replace_all(&text, |c: &regex::Captures<'_>| {
            let version = (c[1].parse::<u32>().unwrap(), c[2].parse::<u32>().unwrap());
            if version <= (major, 0) {
                "valle_enabled"
            } else {
                "valle_disabled"
            }
        });
        let text = retired.replace_all(&text, "valle_disabled");
        let mut text = text.replace("crate::", &format!("crate::backend::v{major}::"));
        text = text.replace("#[macro_export]", "#[allow(unused_macros)]");
        text = text.replace("#[test]", "#[test]\n#[ignore = \"requires a matching FFmpeg runtime; run the selected backend tests explicitly\"]");
        if path == Path::new("src/backend/shared/lib.rs") {
            text = text.replace("#[macro_use]\nextern crate bitflags;", "");
            text = text.replace(
                "pub extern crate ffmpeg_sys_next as sys;",
                &format!("pub use valle_ffmpeg_sys::abi{major} as sys;"),
            );
            text = text.replace("#[cfg(feature = \"image\")]\nextern crate image;", "");
            text = text.replace("extern crate libc;", "");
        }
        let text = text
            .lines()
            .filter(|line| !line.starts_with("#!["))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(output, text).unwrap();
    }
}
