// Shipped in the sys crate: no dependency on sibling workspace sources.
pub fn helper(source: &str, major: u32) -> String {
    let source = source.replace("\r\n", "\n");
    let versions = regex::Regex::new(r#"feature\s*=\s*"ffmpeg_(\d+)_(\d+)""#).unwrap();
    let text = versions.replace_all(&source, |c: &regex::Captures<'_>| {
        if (c[1].parse::<u32>().unwrap(), c[2].parse::<u32>().unwrap()) <= (major, 0) {
            "valle_enabled"
        } else {
            "valle_disabled"
        }
    });
    let unsupported =
        regex::Regex::new(r#"feature\s*=\s*"(?:ffmpeg[^" ]*|ff_api[^" ]*|rpi)""#).unwrap();
    assert!(
        !unsupported.is_match(&text),
        "unhandled helper version cfg: {text}"
    );
    text.replace("crate::", &format!("crate::abi{major}::"))
        .replace("#[macro_export]", "#[allow(unused_macros)]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_line_endings_and_all_baseline_gates() {
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/avutil")).unwrap()
        {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let lf = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
            for major in [7, 8, 9] {
                assert_eq!(helper(&lf, major), helper(&lf.replace('\n', "\r\n"), major));
            }
        }
        let gates = r#"#[cfg(all(feature = "ffmpeg_7_0", feature = "ffmpeg_8_0", feature = "ffmpeg_9_0"))]"#;
        assert_eq!(
            helper(gates, 7),
            "#[cfg(all(valle_enabled, valle_disabled, valle_disabled))]"
        );
        assert_eq!(
            helper(gates, 8),
            "#[cfg(all(valle_enabled, valle_enabled, valle_disabled))]"
        );
        assert_eq!(
            helper(gates, 9),
            "#[cfg(all(valle_enabled, valle_enabled, valle_enabled))]"
        );
    }

    #[test]
    #[should_panic(expected = "unhandled helper version cfg")]
    fn unknown_helper_gate_is_rejected() {
        helper(r#"#[cfg(feature = "ffmpeg_future")]"#, 9);
    }
}
