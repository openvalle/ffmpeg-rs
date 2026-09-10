fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = valle_ffmpeg::init()?;
    println!(
        "FFmpeg {} on {}-{}",
        runtime.version.major(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    for library in &runtime.libraries {
        println!(
            "{}: {}.{}.{} ({})",
            library.name,
            library.version >> 16,
            (library.version >> 8) & 255,
            library.version & 255,
            library.path.display()
        );
    }
    Ok(())
}
