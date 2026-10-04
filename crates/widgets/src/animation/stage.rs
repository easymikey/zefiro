use std::time::Duration;

use kernel::domain::appearance::Animations;
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use tachyonfx::{CellFilter, Effect as Animation, EffectRenderer, RefRect};

use crate::{animation::timings::AnimationTimings, screen::FrameLayout};

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

    fn staged_count(&self) -> usize {
        match self {
            Stage::Running(active) => active.len(),
            Stage::Idle | Stage::Ended => 0,
        }
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

    fn progress_at(&self, area: Rect) -> Option<f32> {
        match self {
            Stage::Running(active) => active
                .iter()
                .find(|(_, rect)| *rect == area)
                .and_then(|(animation, _)| animation.timer())
                .map(|timer| timer.alpha()),
            Stage::Idle | Stage::Ended => None,
        }
    }

    fn advance(self, buffer: &mut Buffer, elapsed: Duration) -> Self {
        let Stage::Running(mut active) = self else {
            return Stage::Idle;
        };
        let bounds = buffer.area;
        active.retain_mut(|(animation, area)| {
            let visible = area.intersection(bounds);
            if visible.is_empty() {
                return false;
            }
            buffer.render_effect(animation, visible, elapsed);
            !animation.done()
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
    last_clock: Duration,
    protected: Vec<Rect>,
    wash_area: Option<Rect>,
    pub(crate) vacated: VacatedAreas,
    pub(crate) live: LiveProtected,
    pub(crate) timings: AnimationTimings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct VacatedAreas {
    pub(crate) overlay: Option<Rect>,
    pub(crate) toast: Option<Rect>,
    pub(crate) selected_row: Option<Rect>,
}

#[derive(Debug, Default)]
pub(crate) struct LiveProtected {
    pub(crate) cover: RefRect,
}

#[derive(Debug, Clone, Copy)]
pub struct Backdrop {
    pub animations: Animations,
    pub layout: FrameLayout,
    pub background: Color,
    pub accent: Color,
    pub volume_fill: Color,
    pub volume_lifted: Color,
    pub wash_from: Color,
}

impl AnimationStage {
    pub fn advance_clock(&mut self, clock: Duration) -> Duration {
        let elapsed = clock.saturating_sub(self.last_clock);
        self.last_clock = clock;
        if !self.stage.is_running() {
            return Duration::ZERO;
        }
        elapsed
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.stage.is_running()
    }

    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.stage.is_animating()
    }

    #[must_use]
    pub fn timings(&self) -> AnimationTimings {
        self.timings
    }

    #[must_use]
    pub fn staged_count(&self) -> usize {
        self.stage.staged_count()
    }

    #[must_use]
    pub fn wash_progress(&self) -> Option<f32> {
        self.stage.progress_at(self.wash_area?)
    }

    pub(crate) fn clear(&mut self) {
        self.stage = Stage::Idle;
        self.wash_area = None;
    }

    pub(crate) fn take_running(&mut self) -> Vec<(Animation, Rect)> {
        std::mem::take(&mut self.stage).take_active()
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

    pub(crate) fn remember_protected(&mut self, layout: FrameLayout) {
        self.protected.clear();
        self.protected.extend(layout.cover);
        self.live.cover.set(layout.cover.unwrap_or(Rect::ZERO));
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
        self.wash_area = Some(area);
        self.stage
            .push((animation.with_filter(self.live_cell_filter()), area));
    }

    #[must_use]
    pub fn cell_filter(&self) -> CellFilter {
        if self.protected.is_empty() {
            return CellFilter::All;
        }
        CellFilter::Static(Box::new(CellFilter::NoneOf(
            self.protected
                .iter()
                .copied()
                .map(CellFilter::Area)
                .collect(),
        )))
    }

    fn live_cell_filter(&self) -> CellFilter {
        CellFilter::NoneOf(vec![CellFilter::RefArea(self.live.cover.clone())])
    }

    pub fn advance(&mut self, buffer: &mut Buffer, elapsed: Duration) {
        self.stage = std::mem::take(&mut self.stage).advance(buffer, elapsed);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{buffer::Buffer, layout::Rect, style::Color};
    use tachyonfx::{Interpolation, fx};

    use crate::animation::stage::Stage;

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

    fn fade(duration: u32) -> tachyonfx::Effect {
        fx::fade_from(
            Color::Black,
            Color::Black,
            (duration, Interpolation::Linear),
        )
    }

    #[test]
    fn an_idle_stage_wants_no_frame_and_stages_nothing() {
        let stage = Stage::default();
        assert!(!stage.is_running());
        assert!(!stage.is_animating());
        assert_eq!(stage.staged_count(), 0);
    }

    #[test]
    fn pushing_an_animation_starts_running_and_counts_it() {
        let mut stage = Stage::default();
        stage.push((fade(900), INSIDE));
        assert!(stage.is_running());
        assert!(stage.is_animating());
        assert_eq!(stage.staged_count(), 1);
    }

    #[test]
    fn pushing_onto_an_already_running_stage_adds_to_it() {
        let mut stage = Stage::default();
        stage.push((fade(900), INSIDE));
        stage.push((fade(900), INSIDE));
        assert_eq!(stage.staged_count(), 2);
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
}
