use std::{
    io,
    mem,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use audio::SpectrumAnalyzer;
use kernel::{Cue, Message, Moment, WindowColorsCmd, domain::ThemeName};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use runtime::{
    CoverDecoded,
    CoverRequest,
    FrameDue,
    FrameInput,
    Painted,
    Receivers,
    ShellEffect,
};
use terminal::{
    Capabilities,
    CoverMotion,
    CoverPlacement,
    CoverRenderer,
    OwnedCoverArt,
    ProbeAnswer,
    TerminalEnvironment,
    UnknownThemeError,
    write_window_colors,
};
use widgets::{
    ActiveTheme,
    AnimationStage,
    Backdrop,
    FrameLayout,
    PlaylistAreas,
    Presence,
    SPECTRUM_BANDS,
    Screen,
    Theme,
    abbreviate_home,
};

use crate::{
    shell::{
        backdrop::{BackdropSources, animation_backdrop},
        cover_fade::{
            CoverArrival,
            cover_arrived_fade,
            cover_outcome,
            cover_sources,
            cover_wash,
        },
        frame_clock::{
            ClockState,
            SpectrumSources,
            animation_frame_due,
            clock_frame_due,
            earliest,
            frame_effect,
            progress_frame_due,
            sleep_frame_due,
            spectrum_frame_due,
        },
        motion::{AdvanceSources, Clearing, Motion, ResizeState},
        view::{self, LaidOutScene, LookReload, Presentation, apply_reload},
        window_colors::{
            PendingWindowColors,
            Wash,
            WindowColorsPlan,
            flush_staged_window_colors_plan,
            window_colors_plan,
        },
    },
    startup::Look,
    toast::{ShellError, toast_message},
};

pub(crate) struct Painter {
    presentation: Presentation,
    pixels: CoverRenderer,
    spectrum_analyzer: SpectrumAnalyzer,
    motion: Motion,
    pending_cues: Vec<Cue>,
    animation_stage: AnimationStage,
    pending_window_colors: PendingWindowColors,
    errors: Vec<ShellError>,
}

impl std::fmt::Debug for Painter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Painter").finish_non_exhaustive()
    }
}

impl Painter {
    pub(crate) fn new(area: Rect, look: Look) -> Self {
        let Capabilities {
            picker,
            pixel_path,
            color_depth,
        } = Capabilities::before_probe(&TerminalEnvironment::current());
        let cell_aspect = terminal::cell_aspect(picker.font_size());
        Self {
            presentation: Presentation {
                theme: Theme::from(look.theme),
                appearance: look.appearance,
                pixel_path,
                color_depth,
                cell_aspect,
                home: dirs::home_dir(),
                music_dir: PathBuf::new(),
                music_dir_label: String::new(),
            },
            pixels: CoverRenderer::new(picker),
            spectrum_analyzer: SpectrumAnalyzer::new(),
            motion: Motion {
                area,
                ..Motion::default()
            },
            pending_cues: Vec::new(),
            animation_stage: AnimationStage::default(),
            pending_window_colors: PendingWindowColors::Idle,
            errors: Vec::new(),
        }
    }

    pub(crate) fn apply_probe_answer(&mut self, answer: ProbeAnswer) {
        self.presentation.pixel_path = answer.pixel_path;
        self.presentation.cell_aspect =
            terminal::cell_aspect(answer.picker.font_size());
        self.pixels.set_picker(answer.picker);
    }

    pub(crate) fn resized(&mut self, area: Rect) {
        self.motion.area = area;
        self.motion.resize = ResizeState::Resized;
    }

    pub(crate) fn take_latest(&mut self, cells: &Receivers) {
        if let Some(theme) = cells.theme.take() {
            self.motion.outgoing_theme_background = Some(
                ActiveTheme::new(
                    &self.presentation.theme,
                    self.presentation.color_depth,
                )
                .window_background(),
            );
            apply_reload(
                &mut self.presentation.theme,
                &mut self.presentation.appearance,
                LookReload::Theme(Arc::unwrap_or_clone(theme)),
            );
        }
        if let Some(appearance) = cells.appearance.take() {
            apply_reload(
                &mut self.presentation.theme,
                &mut self.presentation.appearance,
                LookReload::Appearance(Arc::unwrap_or_clone(appearance)),
            );
        }
        let Some(cover) = cells.cover.take() else {
            return;
        };
        let Ok(decoded) = Arc::try_unwrap(cover) else {
            return;
        };
        if let Some(failure) = self.accept_cover(decoded) {
            self.errors.push(failure);
        }
    }

    fn stage_window_colors(&mut self) -> Option<ShellError> {
        match window_colors_plan(self.presentation.appearance.window.animations) {
            WindowColorsPlan::ApplyNow => self.apply_window_colors(),
            WindowColorsPlan::Defer => {
                self.pending_window_colors = PendingWindowColors::Staged;
                None
            }
        }
    }

    fn flush_staged_window_colors(&mut self) {
        let wash = self
            .animation_stage
            .wash_progress()
            .map_or(Wash::Idle, |_| Wash::Running);
        if wash == Wash::Idle {
            self.motion.outgoing_theme_background = None;
        }
        if flush_staged_window_colors_plan(self.pending_window_colors, wash) {
            self.pending_window_colors = PendingWindowColors::Idle;
            if let Some(failure) = self.apply_window_colors() {
                self.errors.push(failure);
            }
        }
    }

    fn apply_window_colors(&self) -> Option<ShellError> {
        ThemeName::new(self.presentation.theme.name.clone()).map_or_else(
            |_| {
                Some(ShellError::Theme(UnknownThemeError {
                    name: self.presentation.theme.name.clone(),
                }))
            },
            |name| {
                write_window_colors(
                    &WindowColorsCmd::Apply(name),
                    &self.presentation.theme,
                )
                .err()
                .map(ShellError::Theme)
            },
        )
    }

    pub(crate) fn perform_effect(&mut self, effect: &ShellEffect) {
        let failure = match effect {
            ShellEffect::WindowColors(WindowColorsCmd::Apply(_)) => {
                self.stage_window_colors()
            }
            ShellEffect::WindowColors(WindowColorsCmd::Reset) => {
                write_window_colors(&WindowColorsCmd::Reset, &self.presentation.theme)
                    .err()
                    .map(ShellError::Theme)
            }
            ShellEffect::Animate(cue) => {
                self.pending_cues.push(*cue);
                None
            }
        };
        if let Some(failure) = failure {
            self.errors.push(failure);
        }
    }

    pub(crate) fn accept_cover(&mut self, decoded: CoverDecoded) -> Option<ShellError> {
        let path = decoded.path.clone();
        match cover_outcome(decoded) {
            Ok(Some(cover)) => {
                self.record_cover_arrived(&path, CoverArrival::Decoded);
                self.pixels.set_cover(cover);
                None
            }
            Ok(None) => {
                self.record_cover_arrived(&path, CoverArrival::Missing);
                None
            }
            Err(failure) => {
                self.record_cover_arrived(&path, CoverArrival::Missing);
                Some(failure)
            }
        }
    }

    fn backdrop(
        &self,
        layout: FrameLayout,
        cover_art_owner: &OwnedCoverArt,
    ) -> Backdrop {
        animation_backdrop(
            &BackdropSources {
                presentation: &self.presentation,
                mix: self.animation_stage.timings().volume_pulse_mix,
                outgoing_background: self.motion.outgoing_theme_background,
            },
            layout,
            cover_art_owner,
        )
    }

    fn record_cover_arrived(&mut self, path: &Path, arrival: CoverArrival) {
        let current = mem::take(&mut self.motion.cover_fade);
        self.motion.cover_fade = cover_arrived_fade(current, path, arrival);
    }

    pub(crate) fn frame_due(&self, view: &FrameInput<'_>) -> FrameDue {
        let effect = frame_effect(&self.animation_stage, self.cover_motion(view.now));
        let animation = animation_frame_due(effect, self.motion.last_paint);
        let progress = progress_frame_due(
            &view.model.player,
            self.motion.on_screen.progress_bar,
            view.now,
        );
        let clock =
            clock_frame_due(&view.model.player, self.motion.on_screen.clock, view.now);
        let sleep = sleep_frame_due(
            view.sleep_deadline,
            self.motion.on_screen.sleep_label,
            view.now,
        );
        let scheduled = [progress, clock, sleep]
            .into_iter()
            .fold(animation, |due, moment| {
                earliest(due, moment.map_or(FrameDue::Settled, FrameDue::At))
            });
        let spectrum = spectrum_frame_due(
            SpectrumSources {
                player: ClockState::of(&view.model.player),
                shown: self.motion.on_screen.spectrum,
                motion: self.motion.spectrum_motion,
                last_paint: self.motion.last_paint,
            },
            view.now,
        );
        earliest(scheduled, spectrum)
    }

    fn cover_motion(&self, now: Moment) -> CoverMotion {
        let elapsed = if self.motion.started == Moment::default() {
            Duration::ZERO
        } else {
            now.elapsed_since(self.motion.started)
        };
        self.pixels.cover_motion(elapsed)
    }

    fn refresh_music_dir(&mut self, music_dir: &Path) {
        if music_dir == self.presentation.music_dir {
            return;
        }
        self.presentation.music_dir = music_dir.to_path_buf();
        self.presentation.music_dir_label =
            self.presentation.home.as_deref().map_or_else(
                || music_dir.display().to_string(),
                |home| abbreviate_home(music_dir, home),
            );
    }

    fn raw_bands(&mut self, view: &FrameInput<'_>) -> widgets::Spectrum {
        if self.motion.on_screen.spectrum == Presence::Hidden {
            return [0.0; SPECTRUM_BANDS];
        }
        match ClockState::of(&view.model.player) {
            ClockState::Playing => self
                .spectrum_analyzer
                .bands::<SPECTRUM_BANDS>(view.spectrum),
            ClockState::Halted => [0.0; SPECTRUM_BANDS],
        }
    }

    pub(crate) fn paint(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        view: FrameInput<'_>,
    ) -> Result<Painted, io::Error> {
        self.take_latest(view.cells);
        self.refresh_music_dir(view.model.music_dir.as_path());
        let raw = self.raw_bands(&view);
        let sources = AdvanceSources {
            view: &view,
            presentation: &self.presentation,
            pending_cues: &self.pending_cues,
        };
        let (motion, advance) = mem::take(&mut self.motion).advance(&sources, &raw);
        self.motion = motion;
        let LaidOutScene { scene, layout } =
            view::view(&view, &self.presentation, &self.motion);
        let wash =
            cover_wash(self.animation_stage.wash_progress(), self.motion.area.width);
        let cover_art_owner = self.pixels.refresh(cover_sources(
            scene,
            CoverPlacement {
                layout,
                fade: advance.fade,
                wash,
            },
        ));
        let cover_art = cover_art_owner.as_cover_art();
        let elapsed = self.animation_stage.advance_clock(scene.clock);
        let backdrop = self.backdrop(layout, &cover_art_owner);
        if advance.clear == Clearing::Clear {
            terminal.clear()?;
        }
        let (pixels, animation_stage) = (&mut self.pixels, &mut self.animation_stage);
        let cues = mem::take(&mut self.pending_cues);
        terminal.draw(|frame| {
            frame.render_widget(
                &Screen {
                    scene,
                    layout: &layout,
                    cover_art,
                },
                frame.area(),
            );
            pixels.place(frame.buffer_mut(), &layout);
            animation_stage.play(cues, &backdrop);
            animation_stage.advance(frame.buffer_mut(), elapsed);
        })?;
        self.flush_staged_window_colors();
        let errors = drain_errors(&mut self.errors);
        Ok(painted(advance.cover, advance.visible_rows, errors))
    }
}

pub(crate) fn resized_area(width: u16, height: u16) -> Rect {
    Rect::new(0, 0, width, height)
}

fn painted(
    cover: Option<CoverRequest>,
    visible_rows: Option<usize>,
    errors: Vec<Message>,
) -> Painted {
    Painted {
        cover,
        visible_rows,
        failures: errors,
    }
}

fn drain_errors(errors: &mut Vec<ShellError>) -> Vec<Message> {
    mem::take(errors)
        .into_iter()
        .map(|failure| toast_message(&failure))
        .collect()
}

fn playlist_body_height(playlist: Option<PlaylistAreas>) -> u16 {
    playlist.map_or(0, |areas| areas.body.height)
}

pub(crate) fn record_playlist_height(
    current: u16,
    playlist: Option<PlaylistAreas>,
) -> (u16, Option<usize>) {
    let updated = playlist_body_height(playlist);
    let visible_rows = (updated != current).then_some(usize::from(updated));
    (updated, visible_rows)
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
        time::Duration,
    };

    use audio::SpectrumTap;
    use config::{AppearanceFile, CoverStyle};
    use crossterm::event::Event;
    use kernel::{Moment, domain::Model};
    use ratatui::{
        Terminal,
        TerminalOptions,
        Viewport,
        backend::CrosstermBackend,
        layout::Rect,
    };
    use rstest::rstest;
    use runtime::{FrameInput, Reaction, Shell as _};
    use terminal::UnknownThemeError;
    use widgets::{OnScreen, PlaylistAreas, Presence, SPECTRUM_BANDS, SpectrumMotion};

    use crate::{
        shell::{
            Shell,
            ShellInput,
            cover_fade::{CoverWanted, wanted_cover},
            frame_clock::ClockState,
            input::message_for,
            motion::{Clearing, Motion, ResizeState, SpectrumAdvance, resize_clearing},
            painter::{
                Painter,
                drain_errors,
                playlist_body_height,
                record_playlist_height,
                resized_area,
            },
            view::{self, LaidOutScene, fallback_theme_file},
        },
        startup::Look,
        toast::{ShellError, toast_message},
    };

    #[test]
    fn no_playlist_area_gives_a_zero_page() {
        assert_eq!(playlist_body_height(None), 0);
    }

    #[test]
    fn the_page_size_is_the_playlist_body_height() {
        let areas = PlaylistAreas {
            pane: Rect::default(),
            body: Rect::new(0, 0, 10, 7),
            rows: Rect::default(),
            scrollbar: Rect::default(),
            selected: None,
        };

        assert_eq!(playlist_body_height(Some(areas)), 7);
    }

    #[test]
    fn no_failures_drain_to_an_empty_list() {
        let mut errors = Vec::new();

        assert_eq!(drain_errors(&mut errors), Vec::new());
    }

    #[test]
    fn two_failures_drain_as_two_toasts_in_order() {
        let first = ShellError::Cover("first".to_string());
        let second = ShellError::Theme(UnknownThemeError {
            name: "gone".to_string(),
        });
        let mut errors = vec![first.clone(), second.clone()];

        assert_eq!(
            drain_errors(&mut errors),
            vec![toast_message(&first), toast_message(&second)]
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn a_resize_event_sets_the_area_and_repaints() {
        let message = message_for(ShellInput::Terminal(Event::Resize(100, 40)));

        assert_eq!(message, Reaction::Repaint);
        assert_eq!(resized_area(100, 40), Rect::new(0, 0, 100, 40));
    }

    #[test]
    fn advancing_requests_a_new_track_cover_once() {
        let mut wanted = None;
        let wants = CoverWanted {
            current: Some(Path::new("/music/track.jpg")),
            style: CoverStyle::Vinyl,
            side: 160,
        };

        let first = wanted_cover(&mut wanted, &wants);
        let second = wanted_cover(&mut wanted, &wants);

        assert_eq!(
            first.map(|request| request.path),
            Some(PathBuf::from("/music/track.jpg"))
        );
        assert_eq!(second, None);
    }

    #[rstest]
    #[case::same_track_same_style_wants_nothing(
        Some(PathBuf::from("/music/track.jpg")),
        CoverStyle::Vinyl
    )]
    #[case::an_off_style_wants_nothing(None, CoverStyle::Off)]
    fn a_cover_is_wanted_only_for_a_new_track_or_style(
        #[case] mut wanted: Option<PathBuf>,
        #[case] style: CoverStyle,
    ) {
        let wants = CoverWanted {
            current: Some(Path::new("/music/track.jpg")),
            style,
            side: 160,
        };

        let request = wanted_cover(&mut wanted, &wants);

        assert_eq!(request, None);
    }

    fn playlist_areas(height: u16) -> Option<PlaylistAreas> {
        Some(PlaylistAreas {
            pane: Rect::default(),
            body: Rect::new(0, 0, 10, height),
            rows: Rect::default(),
            scrollbar: Rect::default(),
            selected: None,
        })
    }

    #[rstest]
    #[case::a_grown_playlist_reports_the_new_height(0, playlist_areas(7), Some(7))]
    #[case::an_unchanged_height_reports_nothing(7, playlist_areas(7), None)]
    fn the_visible_rows_change_only_when_the_playlist_height_does(
        #[case] current: u16,
        #[case] playlist: Option<PlaylistAreas>,
        #[case] expected: Option<usize>,
    ) {
        let (_, visible_rows) = record_playlist_height(current, playlist);

        assert_eq!(visible_rows, expected);
    }

    #[rstest]
    #[case::a_resize_clears_once(ResizeState::Resized, Clearing::Clear)]
    #[case::a_settled_frame_keeps(ResizeState::Clean, Clearing::Keep)]
    fn a_resize_clears_the_screen_once(
        #[case] resize: ResizeState,
        #[case] expected: Clearing,
    ) {
        let (next, clear) = resize_clearing(resize);

        assert_eq!(clear, expected);
        assert_eq!(next, ResizeState::Clean);
    }

    fn spectrum_shown() -> OnScreen {
        OnScreen {
            progress_bar: None,
            clock: Presence::Hidden,
            sleep_label: Presence::Hidden,
            spectrum: Presence::Shown,
        }
    }

    #[test]
    fn advancing_moves_the_spectrum_with_the_player() {
        let mut motion = Motion {
            on_screen: spectrum_shown(),
            ..Motion::default()
        };
        let mut now = Moment::new(Duration::from_millis(16));
        motion.advance_spectrum(
            SpectrumAdvance {
                playback: ClockState::Playing,
                now,
            },
            &[1.0; SPECTRUM_BANDS],
        );
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Moving);

        for _ in 0..60 {
            now = Moment::new(now.since_epoch() + Duration::from_millis(16));
            motion.advance_spectrum(
                SpectrumAdvance {
                    playback: ClockState::Halted,
                    now,
                },
                &[0.0; SPECTRUM_BANDS],
            );
        }
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Settled);
    }

    #[test]
    fn a_resize_reaches_the_painter_and_the_next_frame_uses_the_new_area() {
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(resized_area(80, 24)),
            },
        )
        .unwrap();
        let look = Look {
            theme: fallback_theme_file(),
            appearance: AppearanceFile::default(),
        };
        let mut shell = Shell {
            terminal: &mut terminal,
            frame: Painter::new(resized_area(80, 24), look),
        };
        let model = Model::default();
        let (_writers, cells, _doorbell) = runtime::cells();
        let spectrum = SpectrumTap::silent();

        let reaction = shell.input(ShellInput::Terminal(Event::Resize(120, 40)));

        let stock = FrameInput {
            model: &model,
            spectrum: &spectrum,
            cells: &cells,
            sleep_deadline: None,
            now: Moment::new(Duration::from_secs(5)),
        };
        let LaidOutScene { layout, .. } =
            view::view(&stock, &shell.frame.presentation, &shell.frame.motion);
        assert_eq!(reaction, Reaction::Repaint);
        assert_eq!(layout.screen, Rect::new(0, 0, 120, 40));
        assert_eq!(shell.frame.motion.resize, ResizeState::Resized);
    }
}
