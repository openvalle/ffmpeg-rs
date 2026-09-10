#![cfg(all(feature = "format", feature = "software-scaling"))]
use std::process::Command;
use valle_ffmpeg::{VideoDecoder, VideoEncoder, VideoEncoderOptions, VideoFrame};

fn worker(mode: &str, directory: &std::path::Path, major: Option<u32>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "runtime_worker", "--nocapture"])
        .env("VALLE_FFMPEG_TEST_MODE", mode)
        .env("VALLE_FFMPEG_DIR", directory);
    if let Some(major) = major {
        command.env("VALLE_FFMPEG_TEST_MAJOR", major.to_string());
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "mode {mode}:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_runtime_is_recoverable_error() {
    let directory = tempfile::tempdir().unwrap();
    worker("missing", directory.path(), None);
}

#[test]
#[ignore = "set VALLE_TEST_FFMPEG_7_DIR, VALLE_TEST_FFMPEG_8_DIR and VALLE_TEST_FFMPEG_9_DIR"]
fn native_runtime_matrix() {
    for major in [7, 8, 9] {
        let path = std::env::var_os(format!("VALLE_TEST_FFMPEG_{major}_DIR"))
            .expect("runtime directory missing");
        worker("roundtrip", std::path::Path::new(&path), Some(major));
    }
}

#[test]
#[ignore = "subprocess entry point"]
fn runtime_worker() {
    let Ok(mode) = std::env::var("VALLE_FFMPEG_TEST_MODE") else {
        return;
    };
    if mode == "missing" {
        let error = valle_ffmpeg::init()
            .err()
            .expect("an explicit empty directory must never fall back to system FFmpeg");
        assert!(error.to_string().contains("unavailable or incompatible"));
        assert!(VideoFrame::from_rgba(0, 8, &[]).is_err());
        assert!(VideoDecoder::open("not-a-video.mp4").is_err());
        return;
    }
    let directory = std::env::var_os("VALLE_FFMPEG_DIR").unwrap();
    let empty = tempfile::tempdir().unwrap();
    valle_ffmpeg::set_directory(empty.path().into()).unwrap();
    assert!(valle_ffmpeg::init().is_err());
    valle_ffmpeg::set_directory(directory.into()).unwrap();
    let expected: u32 = std::env::var("VALLE_FFMPEG_TEST_MAJOR")
        .unwrap()
        .parse()
        .unwrap();
    let runtime = valle_ffmpeg::init().unwrap();
    assert_eq!(runtime.version.major(), expected);
    assert!(valle_ffmpeg::set_directory(empty.path().into()).is_err());
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| assert_eq!(valle_ffmpeg::init().unwrap().version.major(), expected));
        }
    });
    let codecs = valle_ffmpeg::codecs().unwrap();
    assert!(codecs.iter().any(|c| c.name == "ffv1" && c.encoder));

    assert!(codecs.iter().any(|c| c.name == "ffv1" && c.decoder));
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("roundtrip.mkv");
    // Odd width exercises padded native rows. Distinct alpha/channel values detect layout mistakes.
    let width = 13;
    let height = 8;
    let options = VideoEncoderOptions {
        width,
        height,
        fps_numerator: 30,
        fps_denominator: 1,
        codec: "ffv1".into(),
        pixel_format: "bgra".into(),
        options: vec![],
    };
    let mut encoder = VideoEncoder::create(&path, &options).unwrap();
    let mut expected_pixels = Vec::new();
    for index in 0..6u8 {
        let pixels: Vec<u8> = (0..width * height)
            .flat_map(|p| [index * 23, (p % 256) as u8, 167, (p % 200 + 55) as u8])
            .collect();
        let frame = VideoFrame::from_rgba(width, height, &pixels).unwrap();
        assert_eq!(frame.to_rgba(), pixels);
        encoder.write(&frame).unwrap();
        expected_pixels.push(pixels);
    }
    encoder.finish().unwrap();
    let mut decoder = VideoDecoder::open(&path).unwrap();
    assert!(decoder.time_base().1 > 0);
    let mut frames = Vec::new();
    while let Some(frame) = decoder.next_frame().unwrap() {
        frames.push(frame);
    }
    assert!(decoder.next_frame().unwrap().is_none());
    drop(decoder);
    assert_eq!(frames.len(), 6);
    assert!(frames[0].is_key());
    for (index, frame) in frames.iter().enumerate() {
        assert_eq!((frame.width(), frame.height()), (width, height));
        assert_eq!(
            frame.to_rgba(),
            expected_pixels[index],
            "pixel mismatch on FFmpeg {expected}, frame {index}"
        );
        if index > 0 {
            assert!(frame.pts() > frames[index - 1].pts());
        }
    }
    let mut missing = options.clone();
    missing.codec = "valle_missing_encoder".into();
    assert!(
        VideoEncoder::create(output.path().join("missing.mkv"), &missing)
            .err()
            .unwrap()
            .to_string()
            .contains("unavailable")
    );
    assert!(VideoDecoder::open(output.path().join("missing.mp4")).is_err());
}
