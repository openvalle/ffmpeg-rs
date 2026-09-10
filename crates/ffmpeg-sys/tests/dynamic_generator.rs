// Exercise the actual binding generator with a tiny, controllable runtime. No native FFmpeg needed.
#[path = "../build/dynamic.rs"]
mod dynamic;

#[test]
fn generated_dispatch_checks_required_symbols_and_c_abi_failure_boundary() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("bindings.original.rs"),
        r#"
        unsafe extern "C" {
            pub fn av_frame_alloc() -> *mut u8;
            pub fn av_optional_probe() -> i32;
        }
    "#,
    )
    .unwrap();
    dynamic::generate(directory.path(), 9);
    let source = format!(
        "mod abi9 {{ include!({:?}); }}\n{}",
        directory.path().join("bindings.rs"),
        include_str!("fixtures/dynamic_runtime.rs")
    );
    let source_path = directory.path().join("probe.rs");
    std::fs::write(&source_path, source).unwrap();
    let executable = directory
        .path()
        .join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    let output = std::process::Command::new("rustc")
        .args(["--edition=2024", "-Adead_code"])
        .arg(&source_path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for mode in [
        "available",
        "required-missing",
        "wrong-check",
        "optional-missing",
        "wrong-call",
    ] {
        let output = std::process::Command::new(&executable)
            .arg(mode)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        if mode == "optional-missing" || mode == "wrong-call" {
            assert!(!output.status.success(), "{mode} unexpectedly returned");
            assert!(
                stderr.contains("panic in a function that cannot unwind"),
                "{mode}: {stderr}"
            );
            let reason = if mode == "optional-missing" {
                "does not export av_optional_probe"
            } else {
                "ABI mismatch"
            };
            assert!(stderr.contains(reason), "{mode}: {stderr}");
            assert!(!String::from_utf8_lossy(&output.stdout).contains("unwound"));
        } else {
            assert!(output.status.success(), "{mode}: {stderr}");
        }
    }
}
