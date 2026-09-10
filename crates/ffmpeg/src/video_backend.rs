use super::ff;
use crate::{Error, Result, VideoEncoderOptions};
use std::path::Path;

fn again(error: ff::Error) -> bool {
    matches!(error, ff::Error::Other { errno } if errno == libc::EAGAIN)
}

pub fn rgba(width: u32, height: u32) -> Result<ff::frame::Video> {
    let mut frame = ff::frame::Video::empty();
    unsafe {
        if frame.as_ptr().is_null() {
            return Err(Error::message("cannot allocate video frame"));
        }
    }
    frame.set_width(width);
    frame.set_height(height);
    frame.set_format(ff::format::Pixel::RGBA);
    let result = unsafe { ff::ffi::av_frame_get_buffer(frame.as_mut_ptr(), 32) };
    if result < 0 {
        return Err(ff::Error::from(result).into());
    }
    Ok(frame)
}

pub struct Decoder {
    input: ff::format::context::Input,
    decoder: ff::decoder::Video,
    stream: usize,
    time_base: (i32, i32),
    draining: bool,
    finished: bool,
    scaler: Option<ff::software::scaling::Context>,
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Self> {
        let input = ff::format::input(path)?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or_else(|| Error::message("input has no video stream"))?;
        let index = stream.index();
        let time_base = stream.time_base();
        let decoder = ff::codec::context::Context::from_parameters(stream.parameters())?
            .decoder()
            .video()?;
        Ok(Self {
            input,
            decoder,
            stream: index,
            time_base: (time_base.0, time_base.1),
            draining: false,
            finished: false,
            scaler: None,
        })
    }

    pub fn time_base(&self) -> (i32, i32) {
        self.time_base
    }

    pub fn next_frame(&mut self) -> Result<Option<ff::frame::Video>> {
        if self.finished {
            return Ok(None);
        }
        loop {
            let mut decoded = ff::frame::Video::empty();
            match self.decoder.receive_frame(&mut decoded) {
                Ok(()) => {
                    let width = decoded.width();
                    let height = decoded.height();
                    let rebuild = self.scaler.as_ref().is_none_or(|scaler| {
                        let input = scaler.input();
                        input.width != width
                            || input.height != height
                            || input.format != decoded.format()
                    });
                    if rebuild {
                        self.scaler = Some(ff::software::scaling::Context::get(
                            decoded.format(),
                            width,
                            height,
                            ff::format::Pixel::RGBA,
                            width,
                            height,
                            ff::software::scaling::Flags::BILINEAR,
                        )?);
                    }
                    return convert_to_rgba(&decoded, self.scaler.as_mut().unwrap()).map(Some);
                }
                Err(ff::Error::Eof) => {
                    self.finished = true;
                    return Ok(None);
                }
                Err(error) if again(error) => {
                    if self.draining {
                        return Err(Error::message(
                            "decoder requested input after end of stream",
                        ));
                    }
                    let mut packet = ff::Packet::empty();
                    loop {
                        match packet.read(&mut self.input) {
                            Ok(()) if packet.stream() == self.stream => {
                                self.decoder.send_packet(&packet)?;
                                break;
                            }
                            Ok(()) => continue,
                            Err(ff::Error::Eof) => {
                                self.decoder.send_eof()?;
                                self.draining = true;
                                break;
                            }
                            Err(error) => return Err(error.into()),
                        }
                    }
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

// Keep the timestamp decision after copy_props, which also copies the original pts.
fn convert_to_rgba(
    decoded: &ff::frame::Video,
    scaler: &mut ff::software::scaling::Context,
) -> Result<ff::frame::Video> {
    let mut output = rgba(decoded.width(), decoded.height())?;
    scaler.run(decoded, &mut output)?;
    let result = unsafe { ff::ffi::av_frame_copy_props(output.as_mut_ptr(), decoded.as_ptr()) };
    if result < 0 {
        return Err(ff::Error::from(result).into());
    }
    output.set_pts(decoded.timestamp().or(decoded.pts()));
    Ok(output)
}


pub struct Encoder {
    output: ff::format::context::Output,
    encoder: ff::encoder::Video,
    scaler: ff::software::scaling::Context,
    stream: usize,
    time_base: ff::Rational,
    width: u32,
    height: u32,
    next_pts: i64,
}

impl Encoder {
    pub fn create(path: &Path, options: &VideoEncoderOptions) -> Result<Self> {
        let codec = ff::encoder::find_by_name(&options.codec).ok_or_else(|| {
            Error::message(format!(
                "encoder '{}' is unavailable in this FFmpeg installation",
                options.codec
            ))
        })?;
        let pixel_format = options
            .pixel_format
            .parse::<ff::format::Pixel>()
            .map_err(|_| {
                Error::message(format!("unknown pixel format '{}'", options.pixel_format))
            })?;
        let mut output = ff::format::output(path)?;
        let global_header = output
            .format()
            .flags()
            .contains(ff::format::Flags::GLOBAL_HEADER);
        let mut encoder = ff::codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()?;
        encoder.set_width(options.width);
        encoder.set_height(options.height);
        encoder.set_format(pixel_format);
        encoder.set_time_base((options.fps_denominator, options.fps_numerator));
        encoder.set_frame_rate(Some((options.fps_numerator, options.fps_denominator)));
        if global_header {
            encoder.set_flags(ff::codec::Flags::GLOBAL_HEADER);
        }
        let mut dictionary = ff::Dictionary::new();
        for (key, value) in &options.options {
            dictionary.set(key, value);
        }
        let encoder = encoder.open_as_with(codec, dictionary)?;
        let mut stream = output.add_stream(codec)?;
        stream.set_time_base(encoder.time_base());
        stream.set_parameters(&encoder);
        let index = stream.index();
        output.write_header()?;
        let time_base = output.stream(index).unwrap().time_base();
        let scaler = ff::software::scaling::Context::get(
            ff::format::Pixel::RGBA,
            options.width,
            options.height,
            pixel_format,
            options.width,
            options.height,
            ff::software::scaling::Flags::BILINEAR,
        )?;
        Ok(Self {
            output,
            encoder,
            scaler,
            stream: index,
            time_base,
            width: options.width,
            height: options.height,
            next_pts: 0,
        })
    }

    pub fn write(&mut self, frame: &ff::frame::Video) -> Result<()> {
        if frame.width() != self.width || frame.height() != self.height {
            return Err(Error::message(
                "frame dimensions differ from encoder dimensions",
            ));
        }
        let mut converted = ff::frame::Video::empty();
        self.scaler.run(frame, &mut converted)?;
        converted.set_pts(Some(self.next_pts));
        self.encoder.send_frame(&converted)?;
        self.next_pts += 1;
        self.drain(false)
    }

    fn drain(&mut self, finishing: bool) -> Result<()> {
        let mut packet = ff::Packet::empty();
        loop {
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    packet.set_stream(self.stream);
                    packet.rescale_ts(self.encoder.time_base(), self.time_base);
                    packet.write_interleaved(&mut self.output)?;
                }
                Err(ff::Error::Eof) if finishing => return Ok(()),
                Err(error) if again(error) && !finishing => return Ok(()),
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn finish(mut self) -> Result<()> {
        self.encoder.send_eof()?;
        self.drain(true)?;
        self.output.write_trailer()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the selected FFmpeg runtime"]
    fn converted_frames_preserve_best_effort_pts_and_properties() {
        let mut scaler = ff::software::scaling::Context::get(
            ff::format::Pixel::BGRA, 2, 2, ff::format::Pixel::RGBA, 2, 2,
            ff::software::scaling::Flags::BILINEAR,
        ).unwrap();
        for (pts, best, expected) in [
            (None, Some(100), Some(100)),
            (Some(42), Some(84), Some(84)),
            (Some(42), None, Some(42)),
            (None, None, None),
        ] {
            let mut decoded = ff::frame::Video::new(ff::format::Pixel::BGRA, 2, 2);
            decoded.set_pts(pts);
            let stride = decoded.stride(0);
            for y in 0..2 {
                decoded.data_mut(0)[y * stride..y * stride + 8]
                    .copy_from_slice(&[1, 2, 3, 255, 5, 6, 7, 255]);
            }
            unsafe {
                let raw = &mut *decoded.as_mut_ptr();
                raw.best_effort_timestamp = best.unwrap_or(ff::ffi::AV_NOPTS_VALUE);
                raw.flags |= ff::ffi::AV_FRAME_FLAG_KEY;
                raw.color_range = ff::ffi::AVColorRange::AVCOL_RANGE_JPEG;
                let side = ff::ffi::av_frame_new_side_data(
                    decoded.as_mut_ptr(), ff::ffi::AVFrameSideDataType::AV_FRAME_DATA_A53_CC, 3,
                );
                assert!(!side.is_null());
                std::ptr::copy_nonoverlapping([9, 8, 7].as_ptr(), (*side).data, 3);
            }
            let output = convert_to_rgba(&decoded, &mut scaler).unwrap();
            assert_eq!(output.pts(), expected, "source pts={pts:?}, best={best:?}");
            assert_eq!(output.timestamp(), best);
            // FFmpeg 7 also has the legacy key_frame field; preserve both representations.
            assert_eq!(output.is_key(), decoded.is_key());
            assert_eq!(unsafe { (*output.as_ptr()).flags & ff::ffi::AV_FRAME_FLAG_KEY },
                ff::ffi::AV_FRAME_FLAG_KEY);
            assert_eq!(output.format(), ff::format::Pixel::RGBA);
            for y in 0..2 {
                assert_eq!(&output.data(0)[y * output.stride(0)..y * output.stride(0) + 8],
                    &[3, 2, 1, 255, 7, 6, 5, 255]);
            }
            unsafe {
                assert_ne!((*output.as_ptr()).data[0], (*decoded.as_ptr()).data[0]);
                assert_eq!((*output.as_ptr()).color_range, ff::ffi::AVColorRange::AVCOL_RANGE_JPEG);
                let side = ff::ffi::av_frame_get_side_data(
                    output.as_ptr(), ff::ffi::AVFrameSideDataType::AV_FRAME_DATA_A53_CC,
                );
                assert!(!side.is_null());
                assert_eq!(std::slice::from_raw_parts((*side).data, (*side).size), &[9, 8, 7]);
            }
        }
    }
}
