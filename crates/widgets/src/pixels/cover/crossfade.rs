use std::{sync::Arc, time::Duration};

use image::{Rgba, RgbaImage};
use tachyonfx::Interpolation;

use crate::{
    animation::timings::{COVER_CROSSFADE_STEPS, TIMINGS},
    pixels::{cover::CoverMotion, numeric::channel_byte},
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
        let reached = (1..=COVER_CROSSFADE_STEPS)
            .filter(|count| {
                f32::from(*count) <= alpha * f32::from(COVER_CROSSFADE_STEPS)
            })
            .max()
            .unwrap_or(0);
        (reached != self.shown_step).then(|| {
            self.shown_step = reached;
            self.frame(incoming)
        })
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
    fn the_first_frame_of_a_crossfade_is_still_the_old_cover() {
        let mut crossfade = running();
        let incoming = filled_cover(NEW_COVER_PIXEL);

        assert!(crossfade.advance(&incoming, Duration::ZERO).is_none());
        assert_eq!(
            sample(&crossfade.on_screen(&incoming)),
            Some(OLD_COVER_PIXEL)
        );
    }

    #[test]
    fn half_way_through_every_pixel_sits_between_the_two_covers() {
        let mut crossfade = running();

        let shown = crossfade.advance(&filled_cover(NEW_COVER_PIXEL), whole() / 2);
        assert!(matches!(
            shown.as_ref().and_then(sample),
            Some(Rgba([red, green, blue, 255]))
                if (21..200).contains(&red)
                    && (41..160).contains(&green)
                    && (41..220).contains(&blue)
        ));
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

    #[test]
    fn a_crossfade_moves_until_its_last_step() {
        let mut crossfade = running();
        let incoming = filled_cover(NEW_COVER_PIXEL);

        crossfade.advance(&incoming, whole() / 2);
        assert_eq!(crossfade.motion(), CoverMotion::Moving);
    }
}
