use std::{sync::Arc, time::Duration};

use image::{Pixel, Rgba, RgbaImage};
use tachyonfx::Interpolation;

use crate::{
    animation::timings::{COVER_CROSSFADE_STEPS, TIMINGS},
    pixels::{
        cover::CoverMotion,
        numeric::{channel_byte, floor},
    },
};

#[derive(Debug)]
pub(crate) struct CoverCrossfade {
    outgoing: Arc<RgbaImage>,
    start: Duration,
    duration: Duration,
    interpolation: Interpolation,
    shown_step: u8,
}

impl CoverCrossfade {
    pub(crate) fn begin(outgoing: Arc<RgbaImage>, since_first_paint: Duration) -> Self {
        let (millis, interpolation) = TIMINGS.cover_crossfade;
        Self {
            outgoing,
            start: since_first_paint,
            duration: Duration::from_millis(u64::from(millis)),
            interpolation,
            shown_step: 0,
        }
    }

    pub(crate) fn advance(
        &mut self,
        incoming: &RgbaImage,
        since_first_paint: Duration,
    ) -> Option<RgbaImage> {
        let whole = self.duration.as_secs_f32();
        let elapsed = since_first_paint.saturating_sub(self.start).as_secs_f32();
        let alpha = if whole <= 0.0 {
            1.0
        } else {
            self.interpolation.alpha((elapsed / whole).clamp(0.0, 1.0))
        };
        let reached = floor::<u8>(alpha * f32::from(COVER_CROSSFADE_STEPS))
            .min(COVER_CROSSFADE_STEPS);
        if reached == self.shown_step {
            return None;
        }
        self.shown_step = reached;
        Some(self.frame(incoming))
    }

    pub(crate) fn on_screen(&self, incoming: &RgbaImage) -> Arc<RgbaImage> {
        if self.shown_step == 0 {
            Arc::clone(&self.outgoing)
        } else {
            Arc::new(self.frame(incoming))
        }
    }

    pub(crate) fn motion(&self) -> CoverMotion {
        if self.shown_step < COVER_CROSSFADE_STEPS {
            CoverMotion::Moving
        } else {
            CoverMotion::Still
        }
    }

    fn frame(&self, incoming: &RgbaImage) -> RgbaImage {
        let alpha = f32::from(self.shown_step) / f32::from(COVER_CROSSFADE_STEPS);
        let mut frame = incoming.clone();
        if self.shown_step < COVER_CROSSFADE_STEPS {
            for (front, back) in frame.pixels_mut().zip(self.outgoing.pixels()) {
                *front = blend_pixel(*back, *front, alpha);
            }
        }
        frame
    }
}

fn blend_pixel(back: Rgba<u8>, front: Rgba<u8>, alpha: f32) -> Rgba<u8> {
    back.map2(&front, |back, front| {
        let (back, front) = (f32::from(back), f32::from(front));
        channel_byte(alpha.mul_add(front - back, back))
    })
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use image::{Rgba, RgbaImage};

    use crate::{
        animation::timings::TIMINGS,
        pixels::cover::{CoverMotion, crossfade::CoverCrossfade},
    };

    const OLD_COVER_PIXEL: Rgba<u8> = Rgba([200, 40, 40, 255]);
    const NEW_COVER_PIXEL: Rgba<u8> = Rgba([20, 160, 220, 255]);

    fn filled_cover(pixel: Rgba<u8>) -> RgbaImage {
        RgbaImage::from_pixel(2, 2, pixel)
    }

    fn whole() -> Duration {
        Duration::from_millis(u64::from(TIMINGS.cover_crossfade.0))
    }

    fn sample(image: &RgbaImage) -> Option<Rgba<u8>> {
        image.get_pixel_checked(1, 1).copied()
    }

    fn running() -> CoverCrossfade {
        CoverCrossfade::begin(Arc::new(filled_cover(OLD_COVER_PIXEL)), Duration::ZERO)
    }

    #[test]
    fn a_played_out_crossfade_shows_the_new_cover_exactly_and_is_still() {
        let mut crossfade = running();
        let incoming = filled_cover(NEW_COVER_PIXEL);

        let shown = crossfade.advance(&incoming, whole());
        assert_eq!(shown.as_ref().and_then(sample), Some(NEW_COVER_PIXEL));
        assert_eq!(crossfade.motion(), CoverMotion::Still);
        assert!(crossfade.advance(&incoming, whole() * 2).is_none());
    }
}
