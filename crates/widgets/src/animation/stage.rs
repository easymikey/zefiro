use std::{sync::Arc, time::Duration};

use kernel::domain::{appearance::Animations, time::Moment};
use ratatui::{buffer::Buffer, layout::Rect};
use tachyonfx::{CellFilter, Effect as Animation, EffectRenderer, RefRect};

use crate::{
    animation::catalogue::PaintedCell,
    pixels::cover::CoverMotion,
    screen::frame_layout::FrameLayout,
    theme::backdrop_style::BackdropStyle,
};

#[derive(Debug, Default)]
enum Stage {
    #[default]
    Idle,
    Running(Vec<(Animation, Rect)>),
    Ended,
}

impl Stage {
    fn is_running(&self) -> bool {
        matches!(self, Stage::Running(_))
    }

    fn is_animating(&self) -> bool {
        !matches!(self, Stage::Idle)
    }

    fn push(&mut self, animation: (Animation, Rect)) {
        match self {
            Stage::Running(active) => active.push(animation),
            Stage::Idle | Stage::Ended => *self = Stage::Running(vec![animation]),
        }
    }

    fn take_active(self) -> Vec<(Animation, Rect)> {
        match self {
            Stage::Running(active) => active,
            Stage::Idle | Stage::Ended => Vec::new(),
        }
    }

    fn advance(self, buffer: &mut Buffer, elapsed: Duration) -> Self {
        let Stage::Running(mut active) = self else {
            return Stage::Idle;
        };
        active.retain_mut(|(animation, area)| {
            paint_kept(buffer, elapsed)(animation, *area)
        });
        if active.is_empty() {
            Stage::Ended
        } else {
            Stage::Running(active)
        }
    }
}

#[derive(Debug, Default)]
pub struct AnimationStage {
    stage: Stage,
    advanced_to: Duration,
    wash: Option<(Animation, Rect)>,
    pub(crate) vacated_areas: VacatedAreas,
    cover_area: RefRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct VacatedAreas {
    pub(crate) overlay: Option<Rect>,
    pub(crate) toast: Option<Rect>,
    pub(crate) selected_row: Option<Rect>,
}

#[derive(Debug)]
pub struct Backdrop<'a> {
    pub animations: Animations,
    pub layout: FrameLayout<'a>,
    pub style: BackdropStyle,
    pub wash_from: Arc<[PaintedCell]>,
}

impl AnimationStage {
    pub fn advance_to(&mut self, since_first_paint: Duration) -> Duration {
        let elapsed = since_first_paint.saturating_sub(self.advanced_to);
        self.advanced_to = since_first_paint;
        if !self.stage.is_running() && self.wash.is_none() {
            return Duration::ZERO;
        }
        elapsed
    }

    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.stage.is_animating() || self.wash.is_some()
    }

    #[must_use]
    pub fn wash_progress(&self) -> Option<f32> {
        let (animation, _) = self.wash.as_ref()?;
        animation.timer().map(|timer| timer.alpha())
    }

    pub(crate) fn clear(&mut self) {
        self.stage = Stage::Idle;
        self.wash = None;
    }

    pub(crate) fn take_running(&mut self) -> Vec<(Animation, Rect)> {
        std::mem::replace(&mut self.stage, Stage::Idle).take_active()
    }

    pub(crate) fn restore_running(&mut self, running: Vec<(Animation, Rect)>) {
        let current = self.take_running();
        let restaged: Vec<Rect> = current.iter().map(|(_, area)| *area).collect();
        let merged: Vec<(Animation, Rect)> = running
            .into_iter()
            .filter(|(_, area)| !restaged.contains(area))
            .chain(current)
            .collect();
        self.stage = if merged.is_empty() {
            Stage::Idle
        } else {
            Stage::Running(merged)
        };
    }

    pub(crate) fn remember_protected(&self, layout: &FrameLayout<'_>) {
        self.cover_area.set(layout.cover_area.unwrap_or(Rect::ZERO));
    }

    pub fn stage(&mut self, animation: Animation, area: Rect) {
        self.stage
            .push((animation.with_filter(self.cell_filter()), area));
    }

    pub(crate) fn stage_at(&mut self, animation: Animation, area: Option<Rect>) {
        if let Some(area) = area {
            self.stage(animation, area);
        }
    }

    pub(crate) fn stage_whole_screen(&mut self, animation: Animation, area: Rect) {
        self.wash = Some((animation.with_filter(self.cell_filter()), area));
    }

    #[must_use]
    pub(crate) fn cell_filter(&self) -> CellFilter {
        CellFilter::NoneOf(vec![CellFilter::RefArea(self.cover_area.clone())])
    }

    pub fn advance(&mut self, buffer: &mut Buffer, elapsed: Duration) {
        self.stage =
            std::mem::replace(&mut self.stage, Stage::Idle).advance(buffer, elapsed);
        self.wash = self.wash.take().and_then(|(mut animation, area)| {
            paint_kept(buffer, elapsed)(&mut animation, area)
                .then_some((animation, area))
        });
    }
}

fn paint_kept(
    buffer: &mut Buffer,
    elapsed: Duration,
) -> impl FnMut(&mut Animation, Rect) -> bool {
    move |animation: &mut Animation, area: Rect| {
        let visible = area.intersection(buffer.area);
        if visible.is_empty() {
            return false;
        }
        buffer.render_effect(animation, visible, elapsed);
        !animation.done()
    }
}

#[must_use]
pub fn animation_frame_due(
    animation_stage: &AnimationStage,
    cover_motion: CoverMotion,
    next_frame_at: Moment,
) -> Option<Moment> {
    (animation_stage.is_animating() || cover_motion == CoverMotion::Moving)
        .then_some(next_frame_at)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::time::Moment;
    use ratatui::{buffer::Buffer, layout::Rect, style::Color};
    use rstest::rstest;
    use tachyonfx::{Interpolation, fx};

    use crate::{
        animation::stage::{AnimationStage, animation_frame_due},
        pixels::cover::CoverMotion,
    };

    const INSIDE: Rect = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 1,
    };

    fn fade(duration_ms: u32) -> tachyonfx::Effect {
        fx::fade_from(
            Color::Black,
            Color::Black,
            (duration_ms, Interpolation::Linear),
        )
    }

    fn washing() -> AnimationStage {
        let mut animation_stage = AnimationStage::default();
        animation_stage.stage_whole_screen(fade(100), INSIDE);
        animation_stage
    }

    #[test]
    fn a_finished_wash_is_dropped_and_a_later_screen_animation_is_no_wash() {
        let mut buffer = Buffer::empty(INSIDE);
        let mut animation_stage = washing();
        assert!(animation_stage.wash_progress().is_some());
        assert!(animation_stage.is_animating());
        animation_stage.advance(&mut buffer, Duration::from_secs(10));
        assert_eq!(animation_stage.wash_progress(), None);
        animation_stage.stage(fade(900), INSIDE);
        assert_eq!(animation_stage.wash_progress(), None);
    }

    #[rstest]
    #[case::a_running_stage_wants_the_next_frame(washing(), CoverMotion::Still, Some)]
    #[case::a_running_crossfade_wants_the_next_frame(
        AnimationStage::default(),
        CoverMotion::Moving,
        Some
    )]
    #[case::a_settled_stage_wants_no_frame(
        AnimationStage::default(),
        CoverMotion::Still,
        |_| None
    )]
    fn animation_frame_due_answers_the_next_frame_while_something_moves(
        #[case] animation_stage: AnimationStage,
        #[case] cover_motion: CoverMotion,
        #[case] due: fn(Moment) -> Option<Moment>,
    ) {
        let next_frame_at = Moment::new(Duration::from_millis(1_033));

        assert_eq!(
            animation_frame_due(&animation_stage, cover_motion, next_frame_at),
            due(next_frame_at)
        );
    }
}
