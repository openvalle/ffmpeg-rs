use super::Context;
use crate::util::format;
use crate::{ChannelLayout, Error, frame};
#[cfg(feature = "codec")]
use crate::decoder;

impl frame::Audio {
    #[inline]
    pub fn resampler(
        &self,
        format: format::Sample,
        channel_layout: ChannelLayout,
        rate: u32,
    ) -> Result<Context, Error> {
        Context::get(
            self.format(),
            self.channel_layout(),
            unsafe { (*self.as_ptr()).sample_rate as u32 },
            format,
            channel_layout,
            rate,
        )
    }
}

#[cfg(feature = "codec")]
impl decoder::Audio {
    #[inline]
    pub fn resampler(
        &self,
        format: format::Sample,
        channel_layout: ChannelLayout,
        rate: u32,
    ) -> Result<Context, Error> {
        Context::get(
            self.format(),
            self.channel_layout(),
            self.rate(),
            format,
            channel_layout,
            rate,
        )
    }
}
