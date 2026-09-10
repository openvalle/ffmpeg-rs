// One copy of each upstream test, instantiated for all three ABI namespaces.
macro_rules! backend_suite {
    ($backend:ident, $major:literal) => {
        mod $backend {
            use valle_ffmpeg::backend::$backend as ffmpeg;
            const MAJOR: u32 = $major;
            mod audio_frame_slices {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/upstream/audio_frame_slices.rs"
                ));
            }
            #[cfg(feature = "filter")]
            mod buffersink_reuse {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/upstream/buffersink_reuse.rs"
                ));
            }
            #[cfg(feature = "format")]
            mod send_rc_race {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/upstream/send_rc_race.rs"
                ));
            }
            #[cfg(feature = "format")]
            mod stream_io {
                include!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/upstream/stream_io.rs"
                ));
            }
        }
    };
}
backend_suite!(v7, 7);
backend_suite!(v8, 8);
backend_suite!(v9, 9);
