use std::time::Duration;

use image::{Rgba, RgbaImage};
use raster::channel_byte;
use tachyonfx::Interpolation;
use widgets::AnimationTimings;

pub(crate) const CLEAR: Rgba<u8> = Rgba([0, 0, 0, 0]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrossfadeStage {
    Idle,
    Running,
    Over,
}

#[derive(Debug, Default)]
pub(crate) struct CoverCrossfade {
    fade: Option<FadingCover>,
}

#[derive(Debug)]
struct FadingCover {
    outgoing: RgbaImage,
    started: Duration,
    duration: Duration,
    interpolation: Interpolation,
}

impl CoverCrossfade {
    pub(crate) fn begin(&mut self, outgoing: RgbaImage, now: Duration) {
        let (millis, interpolation) = AnimationTimings::default().cover_crossfade;
        self.fade = Some(FadingCover {
            outgoing,
            started: now,
            duration: Duration::from_millis(u64::from(millis)),
            interpolation,
        });
    }

    fn alpha(&self, now: Duration) -> Option<f32> {
        let fading = self.fade.as_ref()?;
        let whole = fading.duration.as_secs_f32();
        if whole <= 0.0 {
            return Some(1.0);
        }
        let elapsed = now.saturating_sub(fading.started).as_secs_f32();
        Some(
            fading
                .interpolation
                .alpha((elapsed / whole).clamp(0.0, 1.0)),
        )
    }

    pub(crate) fn stage(&self, now: Duration) -> CrossfadeStage {
        match self.alpha(now) {
            None => CrossfadeStage::Idle,
            Some(alpha) if alpha < 1.0 => CrossfadeStage::Running,
            Some(_) => CrossfadeStage::Over,
        }
    }

    pub(crate) fn settle(&mut self, now: Duration) {
        if self.stage(now) == CrossfadeStage::Over {
            self.fade = None;
        }
    }

    pub(crate) fn crossfade_at(
        &self,
        incoming: &RgbaImage,
        now: Duration,
    ) -> Option<RgbaImage> {
        let fading = self.fade.as_ref()?;
        let alpha = self.alpha(now)?;
        Some(blended(&fading.outgoing, incoming, alpha))
    }
}

fn blended(outgoing: &RgbaImage, incoming: &RgbaImage, alpha: f32) -> RgbaImage {
    RgbaImage::from_fn(incoming.width(), incoming.height(), |column, row| {
        let front = incoming
            .get_pixel_checked(column, row)
            .copied()
            .unwrap_or(CLEAR);
        outgoing
            .get_pixel_checked(column, row)
            .map_or(front, |back| mixed(*back, front, alpha))
    })
}

pub(crate) fn mixed(back: Rgba<u8>, front: Rgba<u8>, alpha: f32) -> Rgba<u8> {
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
    use std::time::Duration;

    use image::{Rgba, RgbaImage};
    use widgets::AnimationTimings;

    use crate::pixels::cover::crossfade::{CoverCrossfade, CrossfadeStage};

    const OLD_COVER_PIXEL: Rgba<u8> = Rgba([200, 128, 40, 255]);
    const NEW_COVER_PIXEL: Rgba<u8> = Rgba([40, 128, 200, 255]);

    fn filled_cover(pixel: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(2, 2, pixel)
    }

    fn whole() -> Duration {
        Duration::from_millis(u64::from(AnimationTimings::default().cover_crossfade.0))
    }

    fn sample(image: &RgbaImage) -> Rgba<u8> {
        image
            .get_pixel_checked(1, 1)
            .copied()
            .unwrap_or(Rgba([0, 0, 0, 0]))
    }

    fn running() -> CoverCrossfade {
        let mut crossfade = CoverCrossfade::default();
        crossfade.begin(filled_cover(OLD_COVER_PIXEL), Duration::ZERO);
        crossfade
    }

    fn shown(crossfade: &CoverCrossfade, now: Duration) -> Option<Rgba<u8>> {
        crossfade
            .crossfade_at(&filled_cover(NEW_COVER_PIXEL), now)
            .map(|image| sample(&image))
    }

    #[test]
    fn the_first_frame_of_a_crossfade_is_still_the_old_cover() {
        let crossfade = running();
        assert_eq!(shown(&crossfade, Duration::ZERO), Some(OLD_COVER_PIXEL));
    }

    #[test]
    fn half_way_through_every_pixel_sits_between_the_two_covers() {
        let crossfade = running();
        assert!(matches!(
            shown(&crossfade, whole() / 2),
            Some(Rgba([red, green, blue, 255]))
                if (1..255).contains(&red)
                    && (1..255).contains(&green)
                    && (1..255).contains(&blue)
        ));
    }

    #[test]
    fn a_played_out_crossfade_shows_the_new_cover_exactly() {
        let crossfade = running();
        assert_eq!(shown(&crossfade, whole()), Some(NEW_COVER_PIXEL));
        assert_eq!(shown(&crossfade, whole() * 2), Some(NEW_COVER_PIXEL));
    }

    #[test]
    fn a_crossfade_settles_once_it_is_played_out() {
        let mut crossfade = running();
        assert_eq!(crossfade.stage(whole() / 2), CrossfadeStage::Running);
        crossfade.settle(whole() / 2);
        assert_eq!(crossfade.stage(whole()), CrossfadeStage::Over);
        crossfade.settle(whole());
        assert_eq!(crossfade.stage(whole()), CrossfadeStage::Idle);
    }

    #[test]
    fn a_cover_with_no_crossfade_behind_it_is_painted_as_it_is() {
        let crossfade = CoverCrossfade::default();
        assert_eq!(shown(&crossfade, Duration::ZERO), None);
    }
}
