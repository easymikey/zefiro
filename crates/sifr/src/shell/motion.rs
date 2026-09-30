use std::{mem, path::PathBuf};

use kernel::{Cue, Moment};
use ratatui::{layout::Rect, style::Color};
use runtime::{CoverRequest, FrameInput};
use terminal::CoverFade;
use widgets::{OnScreen, Presence, Spectrum, SpectrumMotion, SpectrumSmoothing};

use crate::shell::{
    cover_fade::{
        CoverFadePermission,
        CoverWanted,
        resolved_cover_fade,
        track_changed_fade,
        wanted_cover,
    },
    frame_clock::ClockState,
    painter::record_playlist_height,
    view::{Presentation, view},
};

pub(crate) struct Motion {
    pub(in crate::shell) started: Moment,
    pub(in crate::shell) last_paint: Moment,
    pub(in crate::shell) area: Rect,
    pub(in crate::shell) spectrum: SpectrumSmoothing,
    pub(in crate::shell) spectrum_motion: SpectrumMotion,
    pub(in crate::shell) spectrum_at: Moment,
    pub(in crate::shell) playlist_body_height: u16,
    pub(in crate::shell) wanted_cover: Option<PathBuf>,
    pub(in crate::shell) cover_fade: CoverFadePermission,
    pub(in crate::shell) on_screen: OnScreen,
    pub(in crate::shell) resize: ResizeState,
    pub(in crate::shell) outgoing_theme_background: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ResizeState {
    Clean,
    Resized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clearing {
    Clear,
    Keep,
}

pub(crate) struct Advance {
    pub(crate) cover: Option<CoverRequest>,
    pub(crate) visible_rows: Option<usize>,
    pub(crate) fade: CoverFade,
    pub(crate) clear: Clearing,
}

pub(crate) struct AdvanceSources<'a> {
    pub(crate) view: &'a FrameInput<'a>,
    pub(crate) presentation: &'a Presentation,
    pub(crate) pending_cues: &'a [Cue],
}

#[derive(Debug, Clone, Copy)]
pub(in crate::shell) struct SpectrumAdvance {
    pub(in crate::shell) playback: ClockState,
    pub(in crate::shell) now: Moment,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            started: Moment::default(),
            last_paint: Moment::default(),
            area: Rect::default(),
            spectrum: SpectrumSmoothing::default(),
            spectrum_motion: SpectrumMotion::Settled,
            spectrum_at: Moment::default(),
            playlist_body_height: 0,
            wanted_cover: None,
            cover_fade: CoverFadePermission::default(),
            on_screen: OnScreen {
                progress_bar: None,
                clock: Presence::Hidden,
                sleep_label: Presence::Hidden,
                spectrum: Presence::Hidden,
            },
            resize: ResizeState::Clean,
            outgoing_theme_background: None,
        }
    }
}

pub(in crate::shell) fn resize_clearing(
    resize: ResizeState,
) -> (ResizeState, Clearing) {
    match resize {
        ResizeState::Resized => (ResizeState::Clean, Clearing::Clear),
        ResizeState::Clean => (ResizeState::Clean, Clearing::Keep),
    }
}

impl Motion {
    fn record_start(&mut self, now: Moment) {
        if self.started == Moment::default() {
            self.started = now;
        }
    }

    pub(in crate::shell) fn advance_spectrum(
        &mut self,
        advance: SpectrumAdvance,
        raw: &Spectrum,
    ) {
        if self.on_screen.spectrum == Presence::Hidden {
            return;
        }
        let elapsed = advance.now.elapsed_since(self.spectrum_at);
        self.spectrum_at = advance.now;
        let _ = match advance.playback {
            ClockState::Playing => self.spectrum.smooth(raw, elapsed),
            ClockState::Halted => self.spectrum.fade(elapsed),
        };
        self.spectrum_motion = self.spectrum.motion();
    }

    #[must_use]
    pub(crate) fn advance(
        mut self,
        sources: &AdvanceSources<'_>,
        raw: &Spectrum,
    ) -> (Self, Advance) {
        self.record_start(sources.view.now);
        self.advance_spectrum(
            SpectrumAdvance {
                playback: ClockState::of(&sources.view.model.player),
                now: sources.view.now,
            },
            raw,
        );
        self.last_paint = sources.view.now;
        let current_track = sources
            .view
            .model
            .player
            .current()
            .map(|track| track.path());
        let current = mem::take(&mut self.cover_fade);
        let current = track_changed_fade(current, sources.pending_cues, current_track);
        let (cover_fade, fade) = resolved_cover_fade(current);
        self.cover_fade = cover_fade;
        let built = view(sources.view, sources.presentation, &self);
        let cover_style = built.scene.cover_style();
        let on_screen = built.scene.on_screen(&built.layout);
        let playlist = built.layout.playlist;
        let cover = wanted_cover(
            &mut self.wanted_cover,
            &CoverWanted {
                current: current_track,
                style: cover_style,
                side: sources.presentation.appearance.cover.size_px,
            },
        );
        self.on_screen = on_screen;
        let (playlist_body_height, visible_rows) =
            record_playlist_height(self.playlist_body_height, playlist);
        self.playlist_body_height = playlist_body_height;
        let (resize, clear) = resize_clearing(self.resize);
        self.resize = resize;
        (
            self,
            Advance {
                cover,
                visible_rows,
                fade,
                clear,
            },
        )
    }
}
