# valle-ffmpeg-sys

Runtime-loaded FFmpeg 7/8/9 bindings and library validation.

This fork replaces compile-time selection of one FFmpeg ABI with runtime loading
of a compatible FFmpeg 7, 8 or 9 library set. The same executable can use any of
those majors on its target platform. Applications can start without FFmpeg
installed; loading errors are recoverable and retryable. The loader validates
architecture, library versions, required symbols and dependency consistency
before exposing a typed backend.

Documentation and fork rationale: https://github.com/openvalle/ffmpeg-rs#why-this-fork-exists

See NOTICE and LICENSE for source provenance and license terms.
