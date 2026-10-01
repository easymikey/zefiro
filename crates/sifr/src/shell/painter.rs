use std::{
    io::{self, Stdout},
    mem,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use audio::SpectrumAnalyzer;
use config::Animations;
use crossterm::event::Event;
use kernel::{Cue, Message, Moment, Toast, WindowColorsCmd};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};
use runtime::{
    CoverDecoded,
    CoverOutcome,
    CoverRequest,
    Frame,
    FrameDue,
    LatestReceivers,
    Painted,
    Reaction,
    ShellEffect,
};
use terminal::{
    Capabilities,
    CoverMotion,
    CoverRefreshParts,
    CoverRenderer,
    DecodedCover,
    ProbeAnswer,
    TerminalEnvironment,
    write_window_colors,
};
use widgets::{
    ActiveTheme,
    AnimationStage,
    Backdrop,
    CoverArt,
    FrameLayout,
    PixelPath,
    Presence,
    Role,
    SPECTRUM_BANDS,
    Screen,
    Theme,
    abbreviate_home,
};

use crate::{
    shell::{
        cover_crossfade::{
            CoverArrival,
            after_cover_arrival,
            after_track_change,
            cover_wash,
            take_crossfade_permit,
            wanted_cover,
        },
        frame_due::{
            animation_frame_due,
            clock_frame_due,
            progress_frame_due,
            sleep_frame_due,
            spectrum_frame_due,
        },
        input::{self, ShellEvent},
        motion::{Advance, Motion, ScreenClear, SpectrumFeed},
        view::{self, LaidOutScene, Presentation},
    },
    startup::Look,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowColorsWrite {
    Done,
    Staged,
}

pub(crate) struct Painter<'terminal> {
    terminal: &'terminal mut Terminal<CrosstermBackend<Stdout>>,
    presentation: Presentation,
    cover_renderer: CoverRenderer,
    spectrum_analyzer: SpectrumAnalyzer,
    motion: Motion,
    pending_cues: Vec<Cue>,
    animation_stage: AnimationStage,
    window_colors_write: WindowColorsWrite,
    toasts: Vec<Message>,
}

impl std::fmt::Debug for Painter<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Painter").finish_non_exhaustive()
    }
}

impl<'terminal> Painter<'terminal> {
    pub(crate) fn new(
        terminal: &'terminal mut Terminal<CrosstermBackend<Stdout>>,
        look: Look,
        probe_answer: Option<ProbeAnswer>,
    ) -> Self {
        let area = terminal.get_frame().area();
        let Capabilities {
            picker,
            pixel_path,
            color_depth,
        } = Capabilities::before_probe(&TerminalEnvironment::current());
        let (picker, pixel_path) = probe_answer
            .map_or((picker, pixel_path), |answer| {
                (answer.picker, PixelPath::Protocol)
            });
        let cell_aspect = terminal::cell_aspect(picker.font_size());
        Self {
            terminal,
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
            cover_renderer: CoverRenderer::new(picker),
            spectrum_analyzer: SpectrumAnalyzer::new(),
            motion: Motion {
                area,
                ..Motion::default()
            },
            pending_cues: Vec::new(),
            animation_stage: AnimationStage::default(),
            window_colors_write: WindowColorsWrite::Done,
            toasts: Vec::new(),
        }
    }

    fn take_latest(&mut self, latest: &LatestReceivers) {
        if let Some(theme) = latest.theme.take() {
            self.motion.outgoing_theme_background = Some(
                ActiveTheme::new(
                    &self.presentation.theme,
                    self.presentation.color_depth,
                )
                .role(Role::WindowBackground),
            );
            self.presentation.theme = Theme::from(Arc::unwrap_or_clone(theme));
        }
        if let Some(appearance) = latest.appearance.take() {
            self.presentation.appearance = Arc::unwrap_or_clone(appearance);
        }
        let Some(cover) = latest.cover.take() else {
            return;
        };
        let Ok(decoded) = Arc::try_unwrap(cover) else {
            return;
        };
        if let Some(toast) = self.accept_cover(decoded) {
            self.toasts.push(toast);
        }
    }

    fn flush_staged_window_colors(&mut self) {
        if self.animation_stage.wash_progress().is_some() {
            return;
        }
        self.motion.outgoing_theme_background = None;
        if self.window_colors_write == WindowColorsWrite::Staged {
            self.window_colors_write = WindowColorsWrite::Done;
            self.apply_window_colors(&WindowColorsCmd::Apply(
                self.presentation.theme.name.clone(),
            ));
        }
    }

    fn apply_window_colors(&mut self, cmd: &WindowColorsCmd) {
        if let Err(error) = write_window_colors(cmd, &self.presentation.theme) {
            self.toasts
                .push(Message::Toast(Toast::error(error.to_string())));
        }
    }

    fn accept_cover(&mut self, decoded: CoverDecoded) -> Option<Message> {
        let CoverDecoded { path, outcome, .. } = decoded;
        let (arrival, toast) = match outcome {
            CoverOutcome::Art(image) => {
                self.cover_renderer.set_cover(DecodedCover {
                    path: path.clone(),
                    image: Arc::new(image),
                });
                (CoverArrival::Decoded, None)
            }
            CoverOutcome::NoArt => (CoverArrival::Missing, None),
            CoverOutcome::Failed(error) => (
                CoverArrival::Missing,
                Some(Message::Toast(Toast::error(format!("cover art: {error}")))),
            ),
        };
        self.record_cover_arrival(&path, arrival);
        toast
    }

    fn backdrop(&self, layout: FrameLayout, cover_art: &CoverArt) -> Backdrop {
        let theme =
            ActiveTheme::new(&self.presentation.theme, self.presentation.color_depth);
        let fill = theme.colors.role(Role::Accent);
        let background = theme.role(Role::WindowBackground);
        let mix = self.animation_stage.timings().volume_pulse_mix;
        Backdrop {
            animations: self.presentation.appearance.window.animations,
            layout: protected_layout(layout, cover_art),
            background,
            accent: theme.role(Role::Accent),
            volume_fill: theme.role(Role::Accent),
            volume_lifted: theme.lifted(fill, mix),
            wash_from: self.motion.outgoing_theme_background.unwrap_or(background),
        }
    }

    fn advance(&mut self, frame: &Frame<'_>, raw_bands: &widgets::Spectrum) -> Advance {
        self.motion.record_first_paint(frame.now);
        self.motion.last_paint = frame.now;
        self.motion
            .advance_spectrum(SpectrumFeed::of(&frame.model.player), raw_bands);
        let current_track = frame.model.player.current().map(|track| track.path());
        let pending = mem::take(&mut self.motion.pending_crossfade);
        let pending = after_track_change(pending, &self.pending_cues, current_track);
        let (pending, crossfade) = take_crossfade_permit(pending);
        self.motion.pending_crossfade = pending;
        let laid_out = view::view(frame, &self.presentation, &self.motion);
        let cover_style = laid_out.scene.cover_style();
        let on_screen = laid_out.scene.on_screen(&laid_out.layout);
        let body_height = laid_out
            .layout
            .playlist
            .map_or(0, |areas| areas.body.height);
        let size_px = self.presentation.appearance.cover.size_px;
        let cover =
            wanted_cover(&mut self.motion.wanted_cover, current_track, cover_style)
                .map(|path| CoverRequest { path, size_px });
        self.motion.on_screen = on_screen;
        let visible_rows = (body_height != self.motion.playlist_body_height)
            .then_some(usize::from(body_height));
        self.motion.playlist_body_height = body_height;
        Advance {
            cover,
            visible_rows,
            crossfade,
            screen_clear: mem::replace(
                &mut self.motion.screen_clear,
                ScreenClear::NotDue,
            ),
        }
    }

    fn record_cover_arrival(&mut self, path: &Path, arrival: CoverArrival) {
        let current = mem::take(&mut self.motion.pending_crossfade);
        self.motion.pending_crossfade = after_cover_arrival(current, path, arrival);
    }

    fn cover_motion(&self, now: Moment) -> CoverMotion {
        let elapsed = if self.motion.first_paint == Moment::default() {
            Duration::ZERO
        } else {
            now.elapsed_since(self.motion.first_paint)
        };
        self.cover_renderer.cover_motion(elapsed)
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

    fn raw_bands(&mut self, frame: &Frame<'_>) -> widgets::Spectrum {
        if self.motion.on_screen.spectrum == Presence::Hidden {
            return [0.0; SPECTRUM_BANDS];
        }
        match SpectrumFeed::of(&frame.model.player) {
            SpectrumFeed::Live => self
                .spectrum_analyzer
                .bands::<SPECTRUM_BANDS>(frame.spectrum),
            SpectrumFeed::Silent => [0.0; SPECTRUM_BANDS],
        }
    }
}

impl runtime::Shell for Painter<'_> {
    type Input = ShellEvent;
    type Error = io::Error;

    fn input(&mut self, event: Self::Input) -> Reaction {
        if let ShellEvent::Terminal(Event::Resize(width, height)) = &event {
            self.motion.area = Rect::new(0, 0, *width, *height);
            self.motion.screen_clear = ScreenClear::Due;
        }
        input::reaction_for(event)
    }

    fn effect(&mut self, effect: ShellEffect) {
        match &effect {
            ShellEffect::WindowColors(WindowColorsCmd::Apply(_)) => {
                match self.presentation.appearance.window.animations {
                    Animations::Off => self.apply_window_colors(
                        &WindowColorsCmd::Apply(self.presentation.theme.name.clone()),
                    ),
                    Animations::On => {
                        self.window_colors_write = WindowColorsWrite::Staged;
                    }
                }
            }
            ShellEffect::WindowColors(cmd @ WindowColorsCmd::Reset) => {
                self.apply_window_colors(cmd);
            }
            ShellEffect::Animate(cue) => self.pending_cues.push(*cue),
        }
    }

    fn frame_due(&self, frame: &Frame<'_>) -> FrameDue {
        let animation = animation_frame_due(
            &self.animation_stage,
            self.cover_motion(frame.now),
            self.motion.last_paint,
        );
        let progress = progress_frame_due(
            &frame.model.player,
            self.motion.on_screen.progress_bar,
            frame.now,
        );
        let clock = clock_frame_due(
            &frame.model.player,
            self.motion.on_screen.clock,
            frame.now,
        );
        let sleep = sleep_frame_due(
            frame.sleep_deadline,
            self.motion.on_screen.sleep_label,
            frame.now,
        );
        let spectrum =
            spectrum_frame_due(&self.motion, SpectrumFeed::of(&frame.model.player));
        [animation, progress, clock, sleep, spectrum]
            .into_iter()
            .flatten()
            .min()
            .map_or(FrameDue::Settled, FrameDue::At)
    }

    fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Self::Error> {
        self.take_latest(frame.latest);
        self.refresh_music_dir(frame.model.music_dir.as_path());
        let raw_bands = self.raw_bands(&frame);
        let advance = self.advance(&frame, &raw_bands);
        let LaidOutScene { scene, layout } =
            view::view(&frame, &self.presentation, &self.motion);
        let wash =
            cover_wash(self.animation_stage.wash_progress(), self.motion.area.width);
        let cover_art = self.cover_renderer.refresh(
            &scene,
            CoverRefreshParts {
                layout,
                crossfade: advance.crossfade,
                wash,
            },
        );
        let elapsed = self.animation_stage.advance_clock(scene.clock);
        let backdrop = self.backdrop(layout, &cover_art);
        if advance.screen_clear == ScreenClear::Due {
            self.terminal.clear()?;
        }
        let (pixels, animation_stage) =
            (&mut self.cover_renderer, &mut self.animation_stage);
        let cues = mem::take(&mut self.pending_cues);
        self.terminal.draw(|screen| {
            screen.render_widget(
                &Screen {
                    scene,
                    layout: &layout,
                    cover_art: &cover_art,
                },
                screen.area(),
            );
            pixels.place(screen.buffer_mut(), &layout);
            animation_stage.play(cues, &backdrop);
            animation_stage.advance(screen.buffer_mut(), elapsed);
        })?;
        self.flush_staged_window_colors();
        Ok(Painted {
            cover: advance.cover,
            visible_rows: advance.visible_rows,
            toasts: mem::take(&mut self.toasts),
        })
    }
}

fn protected_layout(mut layout: FrameLayout, cover_art: &CoverArt) -> FrameLayout {
    if !matches!(cover_art, CoverArt::Image) {
        layout.cover = None;
    }
    layout
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf, sync::Arc, time::Duration};

    use audio::SpectrumTap;
    use config::AppearanceFile;
    use crossterm::event::Event;
    use image::RgbaImage;
    use kernel::{Cue, Message, Moment, Toast, domain::Model};
    use ratatui::{
        Terminal,
        TerminalOptions,
        Viewport,
        backend::CrosstermBackend,
        buffer::Buffer,
        layout::Rect,
        style::Color,
    };
    use rstest::rstest;
    use runtime::{
        CoverDecoded,
        CoverError,
        CoverOutcome,
        Frame,
        Reaction,
        Shell as _,
    };
    use widgets::{
        AnimationStage,
        Backdrop,
        Breakpoint,
        CoverArt,
        FrameLayout,
        ToastAreas,
    };

    use crate::{
        shell::{
            ShellEvent,
            motion::ScreenClear,
            painter::Painter,
            view::{self, LaidOutScene, test_presentation},
        },
        startup::{Look, fallback_theme_file},
    };

    fn test_terminal() -> Terminal<CrosstermBackend<io::Stdout>> {
        Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
            },
        )
        .unwrap()
    }

    fn test_look() -> Look {
        Look {
            theme: fallback_theme_file(),
            appearance: AppearanceFile::default(),
        }
    }

    fn test_backdrop(cover_art: &CoverArt, outgoing: Option<Color>) -> Backdrop {
        let mut terminal = test_terminal();
        let mut painter = Painter::new(&mut terminal, test_look(), None);
        painter.presentation = test_presentation();
        painter.motion.outgoing_theme_background = outgoing;
        let layout = FrameLayout {
            screen: Rect::new(0, 0, 40, 10),
            breakpoint: Breakpoint::Full,
            content: Rect::default(),
            header: Rect::default(),
            card: None,
            cover: Some(Rect::new(0, 0, 4, 4)),
            playlist_pane: Rect::default(),
            playlist: None,
            key_hints: None,
            search_bounds: Rect::default(),
            overlay: None,
            toast: Some(ToastAreas {
                outer: Rect::new(0, 0, 10, 1),
                painted: Rect::new(0, 0, 10, 1),
            }),
        };
        painter.backdrop(layout, cover_art)
    }

    fn decoded(outcome: CoverOutcome) -> CoverDecoded {
        CoverDecoded {
            path: PathBuf::from("/music/track.jpg"),
            side: 64,
            outcome,
        }
    }

    fn broken_cover_error() -> CoverError {
        CoverError {
            source: image::load_from_memory(b"not an image").unwrap_err(),
        }
    }

    #[rstest]
    #[case::decoded_art(CoverOutcome::Art(RgbaImage::new(2, 2)), None)]
    #[case::no_art(CoverOutcome::NoArt, None)]
    #[case::a_failed_decode(
        CoverOutcome::Failed(broken_cover_error()),
        Some(Message::Toast(Toast::error(format!(
            "cover art: {}",
            broken_cover_error()
        ))))
    )]
    fn an_arriving_cover_raises_a_toast_only_when_the_decode_failed(
        #[case] outcome: CoverOutcome,
        #[case] expected: Option<Message>,
    ) {
        let mut terminal = test_terminal();
        let mut painter = Painter::new(&mut terminal, test_look(), None);

        assert_eq!(painter.accept_cover(decoded(outcome)), expected);
    }

    #[test]
    fn with_no_wash_staged_the_wash_starts_from_the_current_background() {
        let backdrop = test_backdrop(&CoverArt::Missing, None);

        assert_eq!(backdrop.wash_from, backdrop.background);
    }

    #[test]
    fn a_staged_outgoing_background_is_where_the_wash_starts() {
        let outgoing = Color::Rgb(0x11, 0x22, 0x33);

        let backdrop = test_backdrop(&CoverArt::Missing, Some(outgoing));

        assert_eq!(backdrop.wash_from, outgoing);
        assert_ne!(backdrop.wash_from, backdrop.background);
    }

    #[test]
    fn a_pixel_image_cover_stays_protected_from_effects() {
        let backdrop = test_backdrop(&CoverArt::Image, None);

        assert_eq!(backdrop.layout.cover, Some(Rect::new(0, 0, 4, 4)));
    }

    #[test]
    fn a_text_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&CoverArt::Text(Arc::default()), None);

        assert_eq!(backdrop.layout.cover, None);
    }

    #[test]
    fn a_missing_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&CoverArt::Missing, None);

        assert_eq!(backdrop.layout.cover, None);
    }

    #[test]
    fn an_ended_effect_settles_to_no_deadline_after_the_next_paint() {
        let backdrop = test_backdrop(&CoverArt::Missing, None);
        let mut stage = AnimationStage::default();
        stage.play(vec![Cue::ToastRaised], &backdrop);
        assert!(stage.wants_frame(), "sanity: the toast is animating");
        let mut buffer = Buffer::empty(backdrop.layout.screen);

        stage.advance(&mut buffer, Duration::from_secs(10));
        assert!(
            stage.wants_frame(),
            "sanity: the settling frame is still owed once the effect ends"
        );

        stage.play(Vec::new(), &backdrop);
        stage.advance(&mut buffer, Duration::ZERO);

        assert!(!stage.wants_frame());
    }

    #[test]
    fn a_resize_reaches_the_painter_and_the_next_frame_uses_the_new_area() {
        let mut terminal = test_terminal();
        let mut painter = Painter::new(&mut terminal, test_look(), None);
        let model = Model::default();
        let (_senders, latest, _doorbell) = runtime::latest_channels();
        let spectrum = SpectrumTap::silent();

        let reaction = painter.input(ShellEvent::Terminal(Event::Resize(120, 40)));

        let frame = Frame {
            model: &model,
            spectrum: &spectrum,
            latest: &latest,
            sleep_deadline: None,
            now: Moment::new(Duration::from_secs(5)),
        };
        let LaidOutScene { layout, .. } =
            view::view(&frame, &painter.presentation, &painter.motion);
        assert_eq!(reaction, Reaction::Repaint);
        assert_eq!(layout.screen, Rect::new(0, 0, 120, 40));
        assert_eq!(painter.motion.screen_clear, ScreenClear::Due);
    }
}
