use std::{sync::Arc, time::Duration};

use image::{Rgba, RgbaImage};
use tachyonfx::Interpolation;

use crate::{animation::timings::TIMINGS, pixels::numeric::channel_byte};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrossfadeStage {
    Running,
    Over,
}

#[derive(Debug)]
pub(crate) struct CoverCrossfade {
    outgoing: Arc<RgbaImage>,
    started: Duration,
    duration: Duration,
    interpolation: Interpolation,
}

impl CoverCrossfade {
    pub(crate) fn begin(outgoing: Arc<RgbaImage>, now: Duration) -> Self {
        let (millis, interpolation) = TIMINGS.cover_crossfade;
        Self {
            outgoing,
            started: now,
            duration: Duration::from_millis(u64::from(millis)),
            interpolation,
        }
    }

    fn alpha(&self, now: Duration) -> f32 {
        let whole = self.duration.as_secs_f32();
        if whole <= 0.0 {
            return 1.0;
        }
        let elapsed = now.saturating_sub(self.started).as_secs_f32();
        self.interpolation.alpha((elapsed / whole).clamp(0.0, 1.0))
    }

    #[must_use]
    pub(crate) fn stage(&self, now: Duration) -> CrossfadeStage {
        if self.alpha(now) < 1.0 {
            CrossfadeStage::Running
        } else {
            CrossfadeStage::Over
        }
    }

    #[must_use]
    pub(crate) fn crossfade_at(
        &self,
        incoming: &RgbaImage,
        now: Duration,
    ) -> RgbaImage {
        let alpha = self.alpha(now);
        blend_by_column(&self.outgoing, incoming, |_| alpha)
    }
}

pub(crate) fn blend_by_column(
    back: &RgbaImage,
    front: &RgbaImage,
    alpha_at: impl Fn(u32) -> f32,
) -> RgbaImage {
    let mut front = front.clone();
    for (front_row, back_row) in front.rows_mut().zip(back.rows()) {
        for (column, (front_pixel, back_pixel)) in front_row.zip(back_row).enumerate() {
            let alpha = alpha_at(u32::try_from(column).unwrap_or(u32::MAX));
            *front_pixel = blend_pixel(*back_pixel, *front_pixel, alpha);
        }
    }
    front
}

pub(crate) fn blend_pixel(back: Rgba<u8>, front: Rgba<u8>, alpha: f32) -> Rgba<u8> {
    let Rgba(back) = back;
    let Rgba(front) = front;
    Rgba(std::array::from_fn(|channel| {
        let back = f32::from(back.get(channel).copied().unwrap_or(0));
        let front = f32::from(front.get(channel).copied().unwrap_or(0));
        channel_byte(alpha.mul_add(front - back, back))
    }))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use image::{Rgba, RgbaImage};

    use crate::{
        animation::timings::TIMINGS,
        pixels::cover::crossfade::{CoverCrossfade, CrossfadeStage},
    };

    const OLD_COVER_PIXEL: Rgba<u8> = Rgba([200, 128, 40, 255]);
    const NEW_COVER_PIXEL: Rgba<u8> = Rgba([40, 128, 200, 255]);

    fn filled_cover(pixel: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(2, 2, pixel)
    }

    fn whole() -> Duration {
        Duration::from_millis(u64::from(TIMINGS.cover_crossfade.0))
    }

    fn sample(image: &RgbaImage) -> Rgba<u8> {
        image
            .get_pixel_checked(1, 1)
            .copied()
            .unwrap_or(Rgba([0, 0, 0, 0]))
    }

    fn running() -> CoverCrossfade {
        CoverCrossfade::begin(Arc::new(filled_cover(OLD_COVER_PIXEL)), Duration::ZERO)
    }

    fn shown(crossfade: &CoverCrossfade, now: Duration) -> Rgba<u8> {
        sample(&crossfade.crossfade_at(&filled_cover(NEW_COVER_PIXEL), now))
    }

    #[test]
    fn the_first_frame_of_a_crossfade_is_still_the_old_cover() {
        let crossfade = running();
        assert_eq!(shown(&crossfade, Duration::ZERO), OLD_COVER_PIXEL);
    }

    #[test]
    fn half_way_through_every_pixel_sits_between_the_two_covers() {
        let crossfade = running();
        assert!(matches!(
            shown(&crossfade, whole() / 2),
            Rgba([red, green, blue, 255])
                if (1..255).contains(&red)
                    && (1..255).contains(&green)
                    && (1..255).contains(&blue)
        ));
    }

    #[test]
    fn a_played_out_crossfade_shows_the_new_cover_exactly() {
        let crossfade = running();
        assert_eq!(shown(&crossfade, whole()), NEW_COVER_PIXEL);
        assert_eq!(shown(&crossfade, whole() * 2), NEW_COVER_PIXEL);
    }

    #[test]
    fn a_crossfade_is_over_once_it_is_played_out() {
        let crossfade = running();
        assert_eq!(crossfade.stage(whole() / 2), CrossfadeStage::Running);
        assert_eq!(crossfade.stage(whole()), CrossfadeStage::Over);
    }
}
