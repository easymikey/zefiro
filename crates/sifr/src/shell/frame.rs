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
    Cells,
    CoverDecoded,
    CoverRequest,
    FrameDue,
    Painted,
    ShellEffect,
    View,
};
use terminal::{
    Capabilities,
    CoverArtOwner,
    CoverMotion,
    CoverPlacement,
    Pixels,
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
            Playback,
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
        view::{self, Frame, Presentation, Update, install},
        window_colors::{
            PendingWindowColors,
            WindowColorsPlan,
            settle_window_colors_plan,
            window_colors_plan,
        },
    },
    startup::BootLook,
    toast::{ShellFailure, toast_message},
};

pub(crate) struct Painter {
    presentation: Presentation,
    pixels: Pixels,
    spectrum_analyzer: SpectrumAnalyzer,
    motion: Motion,
    pending_cues: Vec<Cue>,
    animation_stage: AnimationStage,
    pending_window_colors: PendingWindowColors,
    failures: Vec<ShellFailure>,
}

impl std::fmt::Debug for Painter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Painter").finish_non_exhaustive()
    }
}

impl Painter {
    pub(crate) fn new(area: Rect, look: BootLook) -> Self {
        let Capabilities {
            picker,
            pixel_path,
            color_depth,
        } = terminal::resolve_immediate(&TerminalEnvironment::current());
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
                music_dir_display: String::new(),
            },
            pixels: Pixels::new(picker),
            spectrum_analyzer: SpectrumAnalyzer::new(),
            motion: Motion {
                area,
                ..Motion::default()
            },
            pending_cues: Vec::new(),
            animation_stage: AnimationStage::default(),
            pending_window_colors: PendingWindowColors::Idle,
            failures: Vec::new(),
        }
    }

    pub(crate) fn adopt(&mut self, answer: ProbeAnswer) {
        self.presentation.pixel_path = answer.pixel_path;
        self.presentation.cell_aspect =
            terminal::cell_aspect(answer.picker.font_size());
        self.pixels.adopt(answer.picker);
    }

    pub(crate) fn resized(&mut self, area: Rect) {
        self.motion.area = area;
        self.motion.resize = ResizeState::Resized;
    }

    pub(crate) fn absorb_cells(&mut self, cells: &Cells) {
        if let Some(theme) = cells.theme.take() {
            self.motion.outgoing_theme_background = Some(
                ActiveTheme::new(
                    &self.presentation.theme,
                    self.presentation.color_depth,
                )
                .window_bg(),
            );
            install(
                &mut self.presentation.theme,
                &mut self.presentation.appearance,
                Update::Theme(Arc::unwrap_or_clone(theme)),
            );
        }
        if let Some(appearance) = cells.appearance.take() {
            install(
                &mut self.presentation.theme,
                &mut self.presentation.appearance,
                Update::Appearance(Arc::unwrap_or_clone(appearance)),
            );
        }
        let Some(cover) = cells.cover.take() else {
            return;
        };
        let Ok(decoded) = Arc::try_unwrap(cover) else {
            return;
        };
        if let Some(failure) = self.cover(decoded) {
            self.failures.push(failure);
        }
    }

    fn stage_window_colors(&mut self) -> Option<ShellFailure> {
        match window_colors_plan(self.presentation.appearance.window.animations) {
            WindowColorsPlan::ApplyNow => self.apply_window_colors(),
            WindowColorsPlan::Defer => {
                self.pending_window_colors = PendingWindowColors::Staged;
                None
            }
        }
    }

    fn settle_window_colors(&mut self) {
        let progress = self.animation_stage.wash_progress();
        if progress.is_none() {
            self.motion.outgoing_theme_background = None;
        }
        if settle_window_colors_plan(self.pending_window_colors, progress) {
            self.pending_window_colors = PendingWindowColors::Idle;
            if let Some(failure) = self.apply_window_colors() {
                self.failures.push(failure);
            }
        }
    }

    fn apply_window_colors(&self) -> Option<ShellFailure> {
        ThemeName::new(self.presentation.theme.name.clone()).map_or_else(
            |_| {
                Some(ShellFailure::Theme(UnknownThemeError {
                    name: self.presentation.theme.name.clone(),
                }))
            },
            |name| {
                write_window_colors(
                    &WindowColorsCmd::Apply(name),
                    &self.presentation.theme,
                )
                .err()
                .map(ShellFailure::Theme)
            },
        )
    }

    pub(crate) fn effect(&mut self, effect: &ShellEffect) {
        let failure = match effect {
            ShellEffect::WindowColors(WindowColorsCmd::Apply(_)) => {
                self.stage_window_colors()
            }
            ShellEffect::WindowColors(WindowColorsCmd::Reset) => {
                write_window_colors(&WindowColorsCmd::Reset, &self.presentation.theme)
                    .err()
                    .map(ShellFailure::Theme)
            }
            ShellEffect::Animate(cue) => {
                self.pending_cues.push(*cue);
                None
            }
        };
        if let Some(failure) = failure {
            self.failures.push(failure);
        }
    }

    pub(crate) fn cover(&mut self, decoded: CoverDecoded) -> Option<ShellFailure> {
        let path = decoded.path.clone();
        match cover_outcome(decoded) {
            Ok(Some(cover)) => {
                self.note_cover_arrived(&path, CoverArrival::Decoded);
                self.pixels.decoded_cover(cover);
                None
            }
            Ok(None) => {
                self.note_cover_arrived(&path, CoverArrival::Missing);
                None
            }
            Err(failure) => {
                self.note_cover_arrived(&path, CoverArrival::Missing);
                Some(failure)
            }
        }
    }

    fn backdrop(
        &self,
        layout: FrameLayout,
        cover_art_owner: &CoverArtOwner,
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

    fn note_cover_arrived(&mut self, path: &Path, arrival: CoverArrival) {
        let current = mem::take(&mut self.motion.cover_fade);
        self.motion.cover_fade = cover_arrived_fade(current, path, arrival);
    }

    pub(crate) fn frame_due(&self, view: &View<'_>) -> FrameDue {
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
                player: Playback::of(&view.model.player),
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
        self.presentation.music_dir_display =
            self.presentation.home.as_deref().map_or_else(
                || music_dir.display().to_string(),
                |home| abbreviate_home(music_dir, home),
            );
    }

    fn raw_bands(&mut self, view: &View<'_>) -> widgets::Spectrum {
        if self.motion.on_screen.spectrum == Presence::Hidden {
            return [0.0; SPECTRUM_BANDS];
        }
        match Playback::of(&view.model.player) {
            Playback::Playing => self
                .spectrum_analyzer
                .bands::<SPECTRUM_BANDS>(view.spectrum),
            Playback::Halted => [0.0; SPECTRUM_BANDS],
        }
    }

    pub(crate) fn paint(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        view: View<'_>,
    ) -> Result<Painted, io::Error> {
        self.absorb_cells(view.cells);
        self.refresh_music_dir(view.model.music_dir.as_path());
        let raw = self.raw_bands(&view);
        let sources = AdvanceSources {
            view: &view,
            presentation: &self.presentation,
            pending_cues: &self.pending_cues,
        };
        let (motion, advance) = mem::take(&mut self.motion).advanced(&sources, &raw);
        self.motion = motion;
        let Frame { scene, layout } =
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
        let elapsed = self.animation_stage.elapsed_since(scene.clock);
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
        self.settle_window_colors();
        let failures = drained_failures(&mut self.failures);
        Ok(painted(advance.cover, advance.viewport, failures))
    }
}

pub(crate) fn resized_area(width: u16, height: u16) -> Rect {
    Rect::new(0, 0, width, height)
}

fn painted(
    cover: Option<CoverRequest>,
    viewport: Option<usize>,
    failures: Vec<Message>,
) -> Painted {
    Painted {
        cover,
        viewport,
        failures,
    }
}

fn drained_failures(failures: &mut Vec<ShellFailure>) -> Vec<Message> {
    mem::take(failures)
        .into_iter()
        .map(|failure| toast_message(&failure))
        .collect()
}

fn playlist_body_height(playlist: Option<PlaylistAreas>) -> u16 {
    playlist.map_or(0, |areas| areas.body.height)
}

pub(crate) fn note_playlist_height(
    current: u16,
    playlist: Option<PlaylistAreas>,
) -> (u16, Option<usize>) {
    let updated = playlist_body_height(playlist);
    let viewport = (updated != current).then_some(usize::from(updated));
    (updated, viewport)
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
    use runtime::{Reaction, Shell as _, View};
    use terminal::UnknownThemeError;
    use widgets::{OnScreen, PlaylistAreas, Presence, SPECTRUM_BANDS, SpectrumMotion};

    use crate::{
        shell::{
            Shell,
            ShellInput,
            cover_fade::{CoverWanted, desired_cover},
            frame::{
                Painter,
                drained_failures,
                note_playlist_height,
                playlist_body_height,
                resized_area,
            },
            frame_clock::Playback,
            input::message_for,
            motion::{Clearing, Motion, ResizeState, SpectrumAdvance, resize_clearing},
            view::{self, Frame, fallback_theme_file},
        },
        startup::BootLook,
        toast::{ShellFailure, toast_message},
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
        let mut failures = Vec::new();

        assert_eq!(drained_failures(&mut failures), Vec::new());
    }

    #[test]
    fn two_failures_drain_as_two_toasts_in_order() {
        let first = ShellFailure::Cover("first".to_string());
        let second = ShellFailure::Theme(UnknownThemeError {
            name: "gone".to_string(),
        });
        let mut failures = vec![first.clone(), second.clone()];

        assert_eq!(
            drained_failures(&mut failures),
            vec![toast_message(&first), toast_message(&second)]
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn a_resize_event_sets_the_area_and_repaints() {
        let message = message_for(ShellInput::Terminal(Event::Resize(100, 40)));

        assert_eq!(message, Reaction::Repaint);
        assert_eq!(resized_area(100, 40), Rect::new(0, 0, 100, 40));
    }

    #[test]
    fn advanced_rows_cover_wants_a_new_track_once() {
        let mut wanted = None;
        let wants = CoverWanted {
            current: Some(Path::new("/music/track.jpg")),
            style: CoverStyle::Vinyl,
            side: 160,
        };

        let first = desired_cover(&mut wanted, &wants);
        let second = desired_cover(&mut wanted, &wants);

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
    fn advanced_rows_cover(
        #[case] mut wanted: Option<PathBuf>,
        #[case] style: CoverStyle,
    ) {
        let wants = CoverWanted {
            current: Some(Path::new("/music/track.jpg")),
            style,
            side: 160,
        };

        let request = desired_cover(&mut wanted, &wants);

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
    fn advanced_rows_viewport(
        #[case] current: u16,
        #[case] playlist: Option<PlaylistAreas>,
        #[case] expected: Option<usize>,
    ) {
        let (_, viewport) = note_playlist_height(current, playlist);

        assert_eq!(viewport, expected);
    }

    #[rstest]
    #[case::a_resize_clears_once(ResizeState::Resized, Clearing::Clear)]
    #[case::a_settled_frame_keeps(ResizeState::Clean, Clearing::Keep)]
    fn advanced_rows_clearing(#[case] resize: ResizeState, #[case] expected: Clearing) {
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
    fn advanced_rows_spectrum() {
        let mut motion = Motion {
            on_screen: spectrum_shown(),
            ..Motion::default()
        };
        let mut now = Moment::new(Duration::from_millis(16));
        motion.advance_spectrum(
            SpectrumAdvance {
                playback: Playback::Playing,
                now,
            },
            &[1.0; SPECTRUM_BANDS],
        );
        assert_eq!(motion.spectrum_motion, SpectrumMotion::Moving);

        for _ in 0..60 {
            now = Moment::new(now.since_epoch() + Duration::from_millis(16));
            motion.advance_spectrum(
                SpectrumAdvance {
                    playback: Playback::Halted,
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
        let look = BootLook {
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

        let stock = View {
            model: &model,
            spectrum: &spectrum,
            cells: &cells,
            sleep_deadline: None,
            now: Moment::new(Duration::from_secs(5)),
        };
        let Frame { layout, .. } =
            view::view(&stock, &shell.frame.presentation, &shell.frame.motion);
        assert_eq!(reaction, Reaction::Repaint);
        assert_eq!(layout.screen, Rect::new(0, 0, 120, 40));
        assert_eq!(shell.frame.motion.resize, ResizeState::Resized);
    }
}
