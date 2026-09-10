use crate::{Error, Result, Version, backend, init};
use std::path::Path;

mod v7 {
    include!(concat!(env!("OUT_DIR"), "/v7/video_backend.rs"));
}
mod v8 {
    include!(concat!(env!("OUT_DIR"), "/v8/video_backend.rs"));
}
mod v9 {
    include!(concat!(env!("OUT_DIR"), "/v9/video_backend.rs"));
}
enum FrameInner {
    V7(backend::v7::frame::Video),
    V8(backend::v8::frame::Video),
    V9(backend::v9::frame::Video),
}
enum DecoderInner {
    V7(v7::Decoder),
    V8(v8::Decoder),
    V9(v9::Decoder),
}
enum EncoderInner {
    V7(v7::Encoder),
    V8(v8::Encoder),
    V9(v9::Encoder),
}

macro_rules! call {
    ($inner:expr, $enum:ident, $value:ident => $body:expr) => {
        match $inner {
            $enum::V7($value) => $body,
            $enum::V8($value) => $body,
            $enum::V9($value) => $body,
        }
    };
}

fn dimensions(width: u32, height: u32) -> Result<usize> {
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(Error::message(
            "video dimensions must be positive and fit in i32",
        ));
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|size| size.checked_mul(4))
        .ok_or_else(|| Error::message("video frame size overflows this architecture"))
}

/// An owned RGBA frame. Its native allocation and release always use the selected ABI.
pub struct VideoFrame(FrameInner);
impl VideoFrame {
    pub fn from_rgba(width: u32, height: u32, pixels: &[u8]) -> Result<Self> {
        let size = dimensions(width, height)?;
        if pixels.len() != size {
            return Err(Error::message(
                "RGBA buffer length does not match dimensions",
            ));
        }
        let version = init()?.version;
        let mut frame = Self(match version {
            Version::V7 => FrameInner::V7(v7::rgba(width, height)?),
            Version::V8 => FrameInner::V8(v8::rgba(width, height)?),
            Version::V9 => FrameInner::V9(v9::rgba(width, height)?),
        });
        let stride = frame.stride();
        let row = width as usize * 4;
        let data = call!(&mut frame.0, FrameInner, frame => frame.data_mut(0));
        for y in 0..height as usize {
            data[y * stride..y * stride + row].copy_from_slice(&pixels[y * row..(y + 1) * row]);
        }
        Ok(frame)
    }
    pub fn width(&self) -> u32 {
        call!(&self.0, FrameInner, frame => frame.width())
    }
    pub fn height(&self) -> u32 {
        call!(&self.0, FrameInner, frame => frame.height())
    }
    pub fn stride(&self) -> usize {
        call!(&self.0, FrameInner, frame => frame.stride(0))
    }
    pub fn pts(&self) -> Option<i64> {
        call!(&self.0, FrameInner, frame => frame.pts())
    }
    pub fn is_key(&self) -> bool {
        call!(&self.0, FrameInner, frame => frame.is_key())
    }
    /// Borrow the native RGBA plane, including row padding; use `stride()` when reading rows.
    pub fn data(&self) -> &[u8] {
        call!(&self.0, FrameInner, frame => frame.data(0))
    }
    pub fn to_rgba(&self) -> Vec<u8> {
        let row = self.width() as usize * 4;
        let mut pixels = Vec::with_capacity(row * self.height() as usize);
        for y in 0..self.height() as usize {
            pixels.extend_from_slice(&self.data()[y * self.stride()..y * self.stride() + row]);
        }
        pixels
    }
}

/// Sequential video decoding to owned RGBA frames. Timestamps use the input stream time base.
pub struct VideoDecoder(DecoderInner);
impl VideoDecoder {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self(match init()?.version {
            Version::V7 => DecoderInner::V7(v7::Decoder::open(path)?),
            Version::V8 => DecoderInner::V8(v8::Decoder::open(path)?),
            Version::V9 => DecoderInner::V9(v9::Decoder::open(path)?),
        }))
    }
    pub fn time_base(&self) -> (i32, i32) {
        call!(&self.0, DecoderInner, decoder => decoder.time_base())
    }
    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>> {
        Ok(match &mut self.0 {
            DecoderInner::V7(decoder) => {
                decoder.next_frame()?.map(|f| VideoFrame(FrameInner::V7(f)))
            }
            DecoderInner::V8(decoder) => {
                decoder.next_frame()?.map(|f| VideoFrame(FrameInner::V8(f)))
            }
            DecoderInner::V9(decoder) => {
                decoder.next_frame()?.map(|f| VideoFrame(FrameInner::V9(f)))
            }
        })
    }
}

/// Encoder selection is explicit and checked against the user's FFmpeg installation.
#[derive(Clone, Debug)]
pub struct VideoEncoderOptions {
    pub width: u32,
    pub height: u32,
    pub fps_numerator: i32,
    pub fps_denominator: i32,
    pub codec: String,
    pub pixel_format: String,
    pub options: Vec<(String, String)>,
}

/// Constant-frame-rate video output. Each write advances one frame; call finish to flush/trailer.
pub struct VideoEncoder(EncoderInner);
impl VideoEncoder {
    pub fn create(path: impl AsRef<Path>, options: &VideoEncoderOptions) -> Result<Self> {
        dimensions(options.width, options.height)?;
        if options.fps_numerator <= 0 || options.fps_denominator <= 0 {
            return Err(Error::message("frame rate must be positive"));
        }
        let path = path.as_ref();
        Ok(Self(match init()?.version {
            Version::V7 => EncoderInner::V7(v7::Encoder::create(path, options)?),
            Version::V8 => EncoderInner::V8(v8::Encoder::create(path, options)?),
            Version::V9 => EncoderInner::V9(v9::Encoder::create(path, options)?),
        }))
    }
    pub fn write(&mut self, frame: &VideoFrame) -> Result<()> {
        match (&mut self.0, &frame.0) {
            (EncoderInner::V7(encoder), FrameInner::V7(frame)) => encoder.write(frame),
            (EncoderInner::V8(encoder), FrameInner::V8(frame)) => encoder.write(frame),
            (EncoderInner::V9(encoder), FrameInner::V9(frame)) => encoder.write(frame),
            _ => Err(Error::message(
                "frame and encoder belong to different FFmpeg ABIs",
            )),
        }
    }
    pub fn finish(self) -> Result<()> {
        call!(self.0, EncoderInner, encoder => encoder.finish())
    }
}
