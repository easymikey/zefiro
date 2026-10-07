use std::time::Duration;

use kernel::domain::{appearance::Animations, time::Moment};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use tachyonfx::{CellFilter, Effect as Animation, EffectRenderer, RefRect};

use crate::{
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

#[derive(Debug, Clone)]
pub struct Backdrop<'a> {
    pub animations: Animations,
    pub layout: FrameLayout<'a>,
    pub style: BackdropStyle,
    pub wash_from: Color,
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
) -> impl FnMut(&mut Animation, Rect) -> bool + '_ {
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
    use tachyonfx::{Interpolation, fx};

    use crate::{
        animation::stage::{AnimationStage, Stage, animation_frame_due},
        pixels::cover::CoverMotion,
    };

    const INSIDE: Rect = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 1,
    };

    const OUTSIDE: Rect = Rect {
        x: 40,
        y: 40,
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

    #[test]
    fn an_idle_stage_wants_no_frame_and_stages_nothing() {
        let stage = Stage::default();
        assert!(!stage.is_running());
        assert!(!stage.is_animating());
        assert!(stage.take_active().is_empty());
    }

    #[test]
    fn pushing_an_animation_starts_running_and_counts_it() {
        let mut stage = Stage::default();
        stage.push((fade(900), INSIDE));
        assert!(stage.is_running());
        assert!(stage.is_animating());
        assert_eq!(stage.take_active().len(), 1);
    }

    #[test]
    fn pushing_onto_an_already_running_stage_adds_to_it() {
        let mut stage = Stage::default();
        stage.push((fade(900), INSIDE));
        stage.push((fade(900), INSIDE));
        assert_eq!(stage.take_active().len(), 2);
    }

    #[test]
    fn taking_active_empties_a_running_stage_and_returns_what_it_held() {
        let mut stage = Stage::default();
        stage.push((fade(900), INSIDE));
        let active = stage.take_active();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn taking_active_from_an_idle_stage_returns_nothing() {
        assert_eq!(Stage::default().take_active().len(), 0);
    }

    #[test]
    fn advancing_an_area_outside_the_buffer_drops_it_and_settles_idle() {
        let mut buffer = Buffer::empty(INSIDE);
        let stage = Stage::Running(vec![(fade(900), OUTSIDE)]);
        let advanced = stage.advance(&mut buffer, Duration::from_millis(1));
        assert!(!advanced.is_running());
    }

    #[test]
    fn advancing_a_fully_elapsed_animation_ends_the_stage() {
        let mut buffer = Buffer::empty(INSIDE);
        let stage = Stage::Running(vec![(fade(900), INSIDE)]);
        let advanced = stage.advance(&mut buffer, Duration::from_secs(10));
        assert!(
            !advanced.is_running(),
            "a fully elapsed animation must not still be running"
        );
    }

    #[test]
    fn advancing_a_partially_elapsed_animation_keeps_it_running() {
        let mut buffer = Buffer::empty(INSIDE);
        let stage = Stage::Running(vec![(fade(900), INSIDE)]);
        let advanced = stage.advance(&mut buffer, Duration::from_millis(1));
        assert!(advanced.is_running());
    }

    #[test]
    fn a_finished_wash_is_dropped_and_a_later_screen_animation_is_no_wash() {
        let mut buffer = Buffer::empty(INSIDE);
        let mut animation_stage = AnimationStage::default();
        animation_stage.stage_whole_screen(fade(100), INSIDE);
        assert!(animation_stage.wash_progress().is_some());
        assert!(animation_stage.is_animating());
        animation_stage.advance(&mut buffer, Duration::from_secs(10));
        assert_eq!(animation_stage.wash_progress(), None);
        animation_stage.stage(fade(900), INSIDE);
        assert_eq!(animation_stage.wash_progress(), None);
    }

    #[test]
    fn a_running_crossfade_wants_the_next_frame() {
        let animation_stage = AnimationStage::default();
        let next_frame_at = Moment::new(Duration::from_millis(1_033));

        assert_eq!(
            animation_frame_due(&animation_stage, CoverMotion::Moving, next_frame_at),
            Some(next_frame_at)
        );
    }

    #[test]
    fn a_settled_crossfade_wants_no_frame() {
        let animation_stage = AnimationStage::default();
        let next_frame_at = Moment::new(Duration::from_millis(1_033));

        assert_eq!(
            animation_frame_due(&animation_stage, CoverMotion::Still, next_frame_at),
            None
        );
    }
}
