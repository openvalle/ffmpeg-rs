# valle-ffmpeg

Rust FFmpeg bindings that select **FFmpeg 7, 8 or 9 at runtime**. One repository,
two crates, one release version:

| Crate | Responsibility |
| --- | --- |
| `valle-ffmpeg` | Runtime selection, codec discovery, owned RGBA frames, video decoding/encoding, and typed native backend APIs |
| `valle-ffmpeg-sys` | Pinned ABI bindings, shared-library discovery, architecture/version validation, function tables and library lifetime |

Forked from [`ffmpeg-next`](https://github.com/zmwangx/rust-ffmpeg) and
[`ffmpeg-sys-next`](https://github.com/zmwangx/rust-ffmpeg-sys) 9.0.0.

## Why this fork exists

**The goal is to replace binding to one FFmpeg ABI at compile time with loading
one compatible FFmpeg 7, 8 or 9 library set at runtime.** The upstream crates can
be built against several FFmpeg versions, but each build targets one ABI: the
native function signatures and memory layouts for that version. Supporting
multiple versions across separate builds does not let one executable select
between those ABIs at runtime.

This fork addresses three distribution requirements:

1. **One binary across FFmpeg majors.** For a given operating system and
   architecture, the same executable can use a customer's FFmpeg 7, 8 or 9
   installation without being recompiled for that major.
2. **FFmpeg is optional at process startup.** Shared libraries are opened through
   `libloading` when the application requests FFmpeg. Applications can start and
   offer unrelated functionality without FFmpeg installed. Loading returns an
   error if the libraries are missing or incompatible, and a failed load can be
   retried after correcting the installation or search directory.
3. **Validate the library set before use.** The loader checks architecture,
   library majors and minimum versions, required symbols, and dependency
   consistency before exposing the typed backend. These checks reject
   incompatible or mixed library sets before accessing native structures through
   the wrong ABI layout.

The implementation includes three typed ABIs in one build and selects one for
the process lifetime. It calls FFmpeg libraries directly in process. These crates
are not drop-in replacements for the upstream API.

## Use

```toml
[dependencies]
valle-ffmpeg = "0.1.0"
```

Use the Git dependency below until a crates.io release is available.
During local development use `path = "../ffmpeg-rs/crates/ffmpeg"` instead.

```rust,no_run
use valle_ffmpeg::{VideoDecoder, init};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = init()?;
    println!("FFmpeg {}", runtime.version.major());
    let mut decoder = VideoDecoder::open("input.mp4")?;
    while let Some(frame) = decoder.next_frame()? {
        println!("{}x{} at {:?}", frame.width(), frame.height(), frame.pts());
        // frame.data() borrows the native RGBA plane; frame.stride() includes row padding.
    }
    Ok(())
}
```

The native libraries are **not bundled or linked at executable startup**. Users
install shared libraries separately and can set `VALLE_FFMPEG_DIR` to their lib
directory or installation prefix. `set_directory()` overrides the environment.
An explicit directory never falls back to a different installation. Failed loads
can be retried; a successful selection remains fixed for the process lifetime.
Library handles outlive every native frame and codec context.

Search order is explicit directory first, otherwise platform installation paths;
within each location FFmpeg 9, then 8, then 7 is attempted. Windows uses versioned
DLLs in absolute PATH directories, Linux uses standard library directories and the
system loader, and macOS includes Homebrew/MacPorts locations. A standalone static
`ffmpeg` executable does not provide the shared libraries required here.

## Features and interfaces

Defaults enable codec, format, filter, device, software-scaling and software-resampling.
The installation must supply the libraries for the enabled features. For a video
application without capture devices or filters:

```toml
valle-ffmpeg = { version = "0.1.0", default-features = false, features = ["codec", "format", "software-scaling", "software-resampling"] }
```

The root API provides `init`, `set_directory`, `set_log_level`, `codecs`,
`VideoFrame`, `VideoDecoder`, `VideoEncoder` and `VideoEncoderOptions`.
The video encoder accepts RGBA frames at a constant frame rate; call `finish()` to
flush packets and write the container trailer. Codec and output pixel format are
explicit options; missing encoders return errors. The decoder returns owned RGBA
frames with best-effort stream timestamps (falling back to PTS), and frames remain usable after the decoder is dropped.

Integrations needing audio primitives, custom muxing, filters or GPU/native pointers
can use the inherited typed APIs under `backend::{v7,v8,v9}`, selected with
`init()?.version`. These are advanced APIs: objects and pointers must never cross
ABI namespaces. Normal video use does not require caller-side version matches.
The `sys::abi*` modules expose raw unsafe FFI. `sys::abi7::check()` (and its 8/9
counterparts) returns a `LoadError` if that ABI does not match the loaded runtime.
`sys::abi7::has_symbol("av_frame_alloc")` checks both the ABI and function availability;
unknown names and APIs excluded by Cargo features return `false`.

Core symbols are checked during load. Optional platform APIs require a matching
FFmpeg build. The generated call wrappers keep `unsafe extern "C"` signatures for
C callback compatibility: calling the wrong ABI or an absent optional function
**aborts the process**, because a panic cannot unwind through that boundary.
Use the fallible checks before advanced calls; `catch_unwind` cannot recover from
this misuse. The root APIs return load errors before making native calls.
Variadic functions and FFmpeg globals are not exposed as dynamic wrappers.

Library paths supplied through `set_directory`, `VALLE_FFMPEG_DIR` or platform search
must be trusted. Architecture and ABI checks detect incompatible installations;
they do not sandbox native code loaded from those paths.

## Build and validation

Rust 1.88+, libclang, and the target C/C++ SDK are required. Build scripts use only
packaged source and pinned public headers; they do not download FFmpeg, inspect
host FFmpeg headers, run target executables or modify the source tree.

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo package --workspace
```

The default tests cover loader validation and absent-runtime behavior without an
FFmpeg installation. Native tests run separately in isolated processes:

```bash
VALLE_TEST_FFMPEG_7_DIR=/path/to/ffmpeg7/lib \
VALLE_TEST_FFMPEG_8_DIR=/path/to/ffmpeg8/lib \
VALLE_TEST_FFMPEG_9_DIR=/path/to/ffmpeg9/lib \
cargo test -p valle-ffmpeg --test runtime native_runtime_matrix -- --ignored --exact
```

The native matrix checks exact RGBA pixels and alpha, row padding, best-effort PTS,
key flags, side data and color properties, frame ownership, concurrent initialization,
missing codecs and recovery after failed loading. It also runs the inherited unit
tests and four integration suites restored from the pinned ffmpeg-next 9.0.0 archive:
audio plane slices, filter frame reuse, cross-thread ownership, and custom stream I/O.
Each ABI runs in a separate process.

To build the same small native fixtures as CI and run all native tests:

```bash
for major in 7 8 9; do
  python3 tools/build-test-runtime.py "$major" --directory "/tmp/ffmpeg-tests/ffmpeg-$major"
done
python3 tools/test-native.py --root /tmp/ffmpeg-tests
```

The fixtures enable FFV1 and PCM s16le decoding, FFV1 encoding, Matroska/WAV I/O,
image2 muxing, file protocol and overlay/null filters. Windows CI builds DLLs from
the same source hashes under MSYS2 UCRT64, includes their transitive MinGW support
DLLs in the test fixture, and runs the Rust tests as native MSVC executables. CI covers Linux, macOS and Windows; test runtimes are never packaged.

`python3 tools/check-required-symbols.py` compares native function references in the
wrapper and sys helpers against the generated ABI inventories and required list.
It includes imports and macro tokens across source cfg branches, excluding comments
and literals. It is a conservative source check for the current target SDK, not a
proof about arbitrary downstream code or computed symbol lookups. CI runs it on
all three platforms. Source specialization tests exercise LF/CRLF input and reject
unhandled upstream version cfgs.

## Upstream and maintenance

The fork starts from the published upstream 9.0.0 crate archives. The Rust wrapper
and binding helpers retain their original authorship and WTFPL license; Valle's
Rust additions use the same license. The sys crate includes public headers from
FFmpeg 7.0, 8.0 and 9.0 with their LGPL-2.1-or-later notices and license texts.
No FFmpeg native libraries or codec implementations are shipped in these packages.
Exact source archives and SHA-256 hashes are recorded in each package's
[wrapper NOTICE](crates/ffmpeg/NOTICE) and [sys NOTICE](crates/ffmpeg-sys/NOTICE).

The main implementation changes are:

- One shared upstream wrapper source is specialized into typed `v7`, `v8` and `v9`
  modules during the build. The root API handles runtime dispatch for codec
  discovery, owned frames and sequential video encoding/decoding.
- Pinned headers and runtime function tables replace host FFmpeg detection,
  native linking and source-build features. Header configuration is generated for
  the Rust target; the public headers were collected with `configure` followed
  by `make install-headers`.
- Feature boundaries support minimal builds, initialization is guarded for
  concurrent callers, and native tests are selected for the active ABI.

When updating, compare upstream changes against the pinned archives, port fixes
to the shared source and update the affected NOTICE hashes. Adding an FFmpeg major
requires its baseline headers, typed bindings, runtime library mapping and native
regression coverage. Do not add support by relaxing ABI version checks.

## Releases

The manual Release workflow defaults to a dry run. Both packages use the workspace
version. Once reviewed, publish from the same revision, sys first and wrapper second.
Configure the repository's `CARGO_REGISTRY_TOKEN` secret before a real publication.
`python3 tools/publish.py` packages and checks the release plan without uploading;
only `--publish` permits uploads. It requires a clean checkout. On a rerun, an
existing version is skipped only if its registry archive checksum exactly matches
the newly packaged bytes and it is not yanked. Conflicting package bytes stop the
release before uploading either crate. Resume partial releases from the same
revision and toolchain; use a new version for changed contents.

Cargo already waits for index propagation. If an upload ends ambiguously, the
script checks the index with a bounded wait and never blindly repeats the upload.
The wrapper is published only after the expected sys package is visible.
