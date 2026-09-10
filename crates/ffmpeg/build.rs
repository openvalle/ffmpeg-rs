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
            source.replace("\r\n", "\n").replace(
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
        let text = specialize(
            &fs::read_to_string(&path).unwrap(),
            major,
            path == Path::new("src/backend/shared/lib.rs"),
        );
        fs::write(output, text).unwrap();
    }
}

fn specialize(source: &str, major: u32, root: bool) -> String {
    let versions = regex::Regex::new(r#"feature\s*=\s*"ffmpeg_(\d+)_(\d+)""#).unwrap();
    let retired =
        regex::Regex::new(r#"feature\s*=\s*"(?:ff_api_[^"]+|ffmpeg4[123]?|rpi)""#).unwrap();
    // Normalize checkout line endings before matching multiline crate-root declarations.
    let text = source.replace("\r\n", "\n");
    let text = versions.replace_all(&text, |c: &regex::Captures<'_>| {
        let version = (c[1].parse::<u32>().unwrap(), c[2].parse::<u32>().unwrap());
        if version <= (major, 0) {
            "valle_enabled"
        } else {
            "valle_disabled"
        }
    });
    let text = retired.replace_all(&text, "valle_disabled");
    let unsupported =
        regex::Regex::new(r#"feature\s*=\s*"(?:ffmpeg[^" ]*|ff_api[^" ]*|rpi)""#).unwrap();
    assert!(
        !unsupported.is_match(&text),
        "unhandled upstream version cfg: {text}"
    );
    let mut text = text.replace("crate::", &format!("crate::backend::v{major}::"));
    text = text.replace("#[macro_export]", "#[allow(unused_macros)]");
    text = text.replace("#[test]", "#[test]\n#[ignore = \"requires a matching FFmpeg runtime; run the selected backend tests explicitly\"]");
    if root {
        text = text.replace("#[macro_use]\nextern crate bitflags;", "");
        text = text.replace(
            "pub extern crate ffmpeg_sys_next as sys;",
            &format!("pub use valle_ffmpeg_sys::abi{major} as sys;"),
        );
        text = text.replace("#[cfg(feature = \"image\")]\nextern crate image;", "");
        text = text.replace("extern crate libc;", "");
    }
    text.lines()
        .filter(|line| !line.starts_with("#!["))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lf_and_crlf_generate_identical_sources_for_every_abi() {
        fn visit(path: &Path) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let lf = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
                    for major in [7, 8, 9] {
                        let is_root = path.ends_with("shared/lib.rs");
                        let output = specialize(&lf, major, is_root);
                        assert_eq!(
                            output,
                            specialize(&lf.replace('\n', "\r\n"), major, is_root),
                            "{}",
                            path.display()
                        );
                        if is_root {
                            assert!(output.contains(&format!(
                                "pub use valle_ffmpeg_sys::abi{major} as sys;"
                            )));
                            assert!(!output.contains("extern crate"));
                        }
                    }
                }
            }
        }
        visit(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/backend/shared"));
    }

    #[test]
    fn version_gates_use_the_major_baseline() {
        let source = r#"#[cfg(all(feature = "ffmpeg_7_0", not(feature = "ffmpeg_8_1")))]"#;
        assert_eq!(
            specialize(source, 8, false),
            "#[cfg(all(valle_enabled, not(valle_disabled)))]"
        );
        assert_eq!(
            specialize(source, 9, false),
            "#[cfg(all(valle_enabled, not(valle_enabled)))]"
        );
    }

    #[test]
    #[should_panic(expected = "unhandled upstream version cfg")]
    fn unrecognized_version_gate_is_rejected() {
        specialize(r#"#[cfg(feature = "ffmpeg_10")]"#, 9, false);
    }
}
