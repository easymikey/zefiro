use std::{mem, path::Path, sync::Arc, time::Duration};

use audio::spectrum::SpectrumAnalyzer;
use crossterm::event::Event;
use kernel::{
    Cue,
    Diagnostic,
    Message,
    Moment,
    PaintError,
    PaintEvent,
    WindowColorsCmd,
    domain::{ThemeName, appearance::Animations, geometry::Cells},
    update::{Machine, Unhandled},
};
use library::{CoverDecoded, CoverJob};
use ratatui::{Terminal, backend::Backend, layout::Rect};
use runtime::{Frame, FrameDue, LatestReceivers, Painted, Reaction, ShellEffect};
use terminal::{Capabilities, CoverPainter, write_window_colors};
use widgets::{
    ActiveTheme,
    AnimationStage,
    Backdrop,
    BackdropStyle,
    CardCover,
    CoverImage,
    CoverMotion,
    CoverRefresh,
    CrossfadePermit,
    FrameLayout,
    Presence,
    SPECTRUM_BANDS,
    Scene,
    ScreenWidget,
};

use crate::shell::{
    cover_crossfade::{
        CoverArrival,
        CoverWant,
        CrossfadeGate,
        CrossfadeGateMessage,
        cover_wash,
        wanted_cover,
    },
    frame_due::{
        animation_frame_due,
        clock_frame_due,
        progress_frame_due,
        sleep_frame_due,
        spectrum_frame_due,
    },
    input::{self, ShellInput},
    motion::{FrameAdvance, Motion, ScreenClear, SpectrumFeed},
    view::{self, LaidOutScene, ShellPresentation},
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum WindowColorsWrite {
    Done,
    Staged(ThemeName),
}

pub(crate) struct Painter<'terminal, B: Backend> {
    terminal: &'terminal mut Terminal<B>,
    presentation: ShellPresentation,
    cover_renderer: CoverPainter,
    spectrum_analyzer: SpectrumAnalyzer,
    motion: Motion,
    pending_cues: Vec<Cue>,
    animation_stage: AnimationStage,
    window_colors_write: WindowColorsWrite,
    toasts: Vec<Message>,
}

impl<B: Backend> std::fmt::Debug for Painter<'_, B> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Painter").finish_non_exhaustive()
    }
}

impl<'terminal, B: Backend> Painter<'terminal, B> {
    pub(crate) fn new(
        terminal: &'terminal mut Terminal<B>,
        theme: config::TomlTheme,
        capabilities: Capabilities,
    ) -> Self {
        let area = terminal.get_frame().area();
        let Capabilities {
            picker,
            pixel_path,
            color_depth,
        } = capabilities;
        let cell_aspect = terminal::cell_aspect(picker.font_size());
        Self {
            terminal,
            presentation: ShellPresentation {
                theme: crate::startup::theme(theme),
                pixel_path,
                color_depth,
                cell_aspect,
                home: dirs::home_dir(),
                spectrum: [0.0; SPECTRUM_BANDS],
            },
            cover_renderer: CoverPainter::new(picker),
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
            let outgoing = ActiveTheme::new(
                &self.presentation.theme,
                self.presentation.color_depth,
            );
            self.motion.outgoing_theme_background =
                Some(BackdropStyle::from_theme(&outgoing).background);
            self.presentation.theme =
                crate::startup::theme(Arc::unwrap_or_clone(theme));
        }
        let Some(cover) = latest.cover.take() else {
            return;
        };
        if let Some(toast) = self.accept_cover(&cover) {
            self.toasts.push(toast);
        }
    }

    fn flush_staged_window_colors(&mut self) {
        if self.animation_stage.wash_progress().is_some() {
            return;
        }
        self.motion.outgoing_theme_background = None;
        self.write_staged_window_colors();
    }

    fn write_staged_window_colors(&mut self) {
        let cmd = match &self.window_colors_write {
            WindowColorsWrite::Staged(name)
                if *name == self.presentation.theme.name =>
            {
                WindowColorsCmd::Set(name.clone())
            }
            WindowColorsWrite::Staged(_) | WindowColorsWrite::Done => return,
        };
        self.window_colors_write = WindowColorsWrite::Done;
        self.set_window_colors(&cmd);
    }

    fn set_window_colors(&mut self, cmd: &WindowColorsCmd) {
        if let Err(error) = write_window_colors(cmd, &self.presentation.theme) {
            self.toasts.push(paint_error(PaintError::WindowColors(
                Diagnostic::from_error(&error),
            )));
        }
    }

    fn accept_cover(&mut self, decoded: &CoverDecoded) -> Option<Message> {
        let CoverDecoded { path, art, .. } = decoded;
        let (arrival, toast) = match art {
            library::CoverArt::Image(image) => {
                self.cover_renderer.set_cover(CoverImage {
                    path: path.clone(),
                    image: Arc::clone(image),
                });
                (CoverArrival::Decoded, None)
            }
            library::CoverArt::Missing => (CoverArrival::Missing, None),
            library::CoverArt::Error(error) => (
                CoverArrival::Missing,
                Some(paint_error(PaintError::Cover(Diagnostic::from_error(
                    error,
                )))),
            ),
        };
        self.record_cover_arrival(path, arrival);
        toast
    }

    fn backdrop(&self, laid_out: &LaidOutScene<'_>, cover_art: &CardCover) -> Backdrop {
        let theme =
            ActiveTheme::new(&self.presentation.theme, self.presentation.color_depth)
                .with_volume_pulse(self.animation_stage.timings().volume_pulse_mix);
        let style = BackdropStyle::from_theme(&theme);
        Backdrop {
            animations: laid_out.scene.appearance().settings.animations,
            layout: protected_layout(laid_out.layout, cover_art),
            background: style.background,
            accent: style.accent,
            volume_fill: style.volume_fill,
            volume_lifted: style.volume_lifted,
            wash_from: self
                .motion
                .outgoing_theme_background
                .unwrap_or(style.background),
        }
    }

    fn advance_spectrum(&mut self, frame: &Frame<'_>) {
        let feed = SpectrumFeed::of(
            view::scene(frame, &self.presentation, &self.motion).player,
        );
        let raw_bands = self.raw_bands(frame, feed);
        self.motion.record_first_paint(frame.now);
        self.motion.last_paint = frame.now;
        self.presentation.spectrum = self.motion.advance_spectrum(feed, &raw_bands);
    }

    fn record_cover_arrival(&mut self, path: &Path, arrival: CoverArrival) {
        let arrived = CrossfadeGateMessage::CoverArrived {
            path: path.to_path_buf(),
            arrival,
        };
        settle_crossfade_gate(&mut self.motion.crossfade_gate, arrived);
    }

    fn cover_motion(&self, now: Moment) -> CoverMotion {
        let elapsed = self
            .motion
            .first_paint
            .map_or(Duration::ZERO, |first| now.elapsed_since(first));
        self.cover_renderer.cover_motion(elapsed)
    }

    fn raw_bands(
        &mut self,
        frame: &Frame<'_>,
        feed: SpectrumFeed,
    ) -> widgets::Spectrum {
        if self.motion.on_screen.spectrum == Presence::Hidden {
            return [0.0; SPECTRUM_BANDS];
        }
        match feed {
            SpectrumFeed::Live => self
                .spectrum_analyzer
                .bands::<SPECTRUM_BANDS>(frame.spectrum),
            SpectrumFeed::Silent => [0.0; SPECTRUM_BANDS],
        }
    }
}

impl<B> runtime::Shell for Painter<'_, B>
where
    B: Backend,
    B::Error: 'static,
{
    type Input = ShellInput;
    type Error = B::Error;

    fn input(&mut self, event: Self::Input) -> Reaction {
        if let ShellInput::Terminal(Event::Resize(width, height)) = &event {
            self.motion.area = Rect::new(0, 0, *width, *height);
            self.motion.screen_clear = ScreenClear::Due;
        }
        input::reaction_for(event)
    }

    fn effect(&mut self, effect: ShellEffect, animations: Animations) {
        match effect {
            ShellEffect::WindowColors(WindowColorsCmd::Set(name)) => {
                self.window_colors_write = WindowColorsWrite::Staged(name);
                match animations {
                    Animations::Off => self.write_staged_window_colors(),
                    Animations::On => {}
                }
            }
            ShellEffect::WindowColors(cmd @ WindowColorsCmd::Reset) => {
                self.window_colors_write = WindowColorsWrite::Done;
                self.set_window_colors(&cmd);
            }
            ShellEffect::Animate(cue) => self.pending_cues.push(cue),
        }
    }

    fn frame_due(&self, frame: &Frame<'_>) -> FrameDue {
        let scene = view::scene(frame, &self.presentation, &self.motion);
        let animation = animation_frame_due(
            &self.animation_stage,
            self.cover_motion(frame.now),
            self.motion.last_paint,
        );
        let progress = progress_frame_due(
            scene.player,
            self.motion.on_screen.progress_bar,
            frame.now,
        );
        let clock =
            clock_frame_due(scene.player, self.motion.on_screen.clock, frame.now);
        let sleep = sleep_frame_due(
            frame.sleep_deadline,
            self.motion.on_screen.sleep_label,
            frame.now,
        );
        let spectrum = spectrum_frame_due(&self.motion, SpectrumFeed::of(scene.player));
        [animation, progress, clock, sleep, spectrum]
            .into_iter()
            .flatten()
            .min()
            .map_or(FrameDue::Settled, FrameDue::At)
    }

    fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Self::Error> {
        self.take_latest(frame.latest);
        self.advance_spectrum(&frame);
        let laid_out = view::view(&frame, &self.presentation, &self.motion);
        let advance = advance(&mut self.motion, &self.pending_cues, &laid_out);
        let (scene, layout) = (laid_out.scene, laid_out.layout);
        let wash = cover_wash(
            self.animation_stage.wash_progress(),
            Cells(self.motion.area.width),
        );
        let cover_art = self.cover_renderer.refresh(
            &scene,
            CoverRefresh {
                layout,
                crossfade: advance.crossfade,
                wash,
            },
        );
        let elapsed = self.animation_stage.advance_clock(scene.clock);
        let backdrop = self.backdrop(&laid_out, &cover_art);
        if advance.screen_clear == ScreenClear::Due {
            self.terminal.clear()?;
        }
        let (pixels, animation_stage) =
            (&mut self.cover_renderer, &mut self.animation_stage);
        let cues = mem::take(&mut self.pending_cues);
        self.terminal.draw(|screen| {
            screen.render_widget(
                &ScreenWidget {
                    scene,
                    layout: &layout,
                    cover_art: &cover_art,
                },
                screen.area(),
            );
            pixels.paint(screen.buffer_mut(), &layout);
            animation_stage.play(cues, &backdrop);
            animation_stage.advance(screen.buffer_mut(), elapsed);
        })?;
        self.flush_staged_window_colors();
        Ok(Painted {
            cover: advance.cover,
            visible_rows: advance.visible_rows.map(Cells::count),
            toasts: mem::take(&mut self.toasts),
        })
    }
}

fn advance(
    motion: &mut Motion,
    cues: &[Cue],
    laid_out: &LaidOutScene<'_>,
) -> FrameAdvance {
    let LaidOutScene { scene, layout } = laid_out;
    let current_track = scene.current_track_path();
    if cues.contains(&Cue::TrackChanged) {
        let changed =
            CrossfadeGateMessage::TrackChanged(current_track.map(Path::to_path_buf));
        settle_crossfade_gate(&mut motion.crossfade_gate, changed);
    }
    let crossfade = match motion
        .crossfade_gate
        .transition(CrossfadeGateMessage::PermitTaken)
    {
        Ok(cmd) => cmd
            .effects()
            .copied()
            .last()
            .unwrap_or(CrossfadePermit::Withheld),
        Err(Unhandled) => CrossfadePermit::Withheld,
    };
    let cover = cover_job(motion, scene);
    motion.on_screen = scene.on_screen(layout);
    let body_height = layout
        .playlist
        .map_or(Cells(0), |areas| Cells(areas.body.height));
    let visible_rows =
        (body_height != motion.playlist_body_height).then_some(body_height);
    motion.playlist_body_height = body_height;
    FrameAdvance {
        cover,
        visible_rows,
        crossfade,
        screen_clear: mem::replace(&mut motion.screen_clear, ScreenClear::NotDue),
    }
}

fn settle_crossfade_gate(gate: &mut CrossfadeGate, message: CrossfadeGateMessage) {
    let effects = match gate.transition(message) {
        Ok(cmd) => cmd.effects().count(),
        Err(Unhandled) => 0,
    };
    debug_assert_eq!(effects, 0);
}

fn cover_job(motion: &mut Motion, scene: &Scene<'_>) -> Option<CoverJob> {
    let want = wanted_cover(
        motion.wanted_cover.as_deref(),
        scene.current_track_path(),
        scene.cover_mode(),
    );
    match want {
        CoverWant::None => {
            motion.wanted_cover = None;
            None
        }
        CoverWant::Same => None,
        CoverWant::New(path) => {
            motion.wanted_cover = Some(path.clone());
            Some(CoverJob::new(path, scene.appearance().cover_size_px))
        }
    }
}

fn paint_error(error: PaintError) -> Message {
    Message::from(PaintEvent::Error(error))
}

fn protected_layout(layout: FrameLayout, cover_art: &CardCover) -> FrameLayout {
    if matches!(cover_art, CardCover::Image) {
        layout
    } else {
        FrameLayout {
            cover: None,
            ..layout
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use audio::tap::SpectrumTap;
    use crossterm::event::Event;
    use image::RgbaImage;
    use kernel::{
        Diagnostic,
        Message,
        Moment,
        PaintError,
        PaintEvent,
        WindowColorsCmd,
        domain::{Model, ThemeName, appearance::Animations, geometry::Pixels},
    };
    use library::{CoverDecoded, CoverError};
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
    use rstest::rstest;
    use runtime::{Frame, Reaction, Shell as _, ShellEffect};
    use terminal::{Capabilities, TerminalEnvironment};
    use widgets::{Backdrop, Breakpoint, CardCover, FrameLayout, ToastAreas};

    use crate::{
        shell::{
            ShellInput,
            motion::ScreenClear,
            painter::{Painter, WindowColorsWrite},
            view::{self, LaidOutScene, test_presentation},
        },
        startup::fallback_theme,
    };

    fn test_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn test_capabilities() -> Capabilities {
        Capabilities::from_environment(&TerminalEnvironment::default())
    }

    fn test_theme() -> config::TomlTheme {
        fallback_theme()
    }

    fn test_backdrop(cover_art: &CardCover, outgoing: Option<Color>) -> Backdrop {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
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
        let model = Model::default();
        let (_senders, latest, _doorbell) = runtime::latest_channels();
        let spectrum = SpectrumTap::silent();
        let frame = Frame {
            model: &model,
            spectrum: &spectrum,
            latest: &latest,
            sleep_deadline: None,
            now: Moment::default(),
        };
        let mut laid_out = view::view(&frame, &painter.presentation, &painter.motion);
        laid_out.layout = layout;
        painter.backdrop(&laid_out, cover_art)
    }

    fn decoded(art: library::CoverArt) -> CoverDecoded {
        CoverDecoded {
            path: PathBuf::from("/music/track.jpg"),
            side: Pixels(64),
            art,
        }
    }

    fn broken_cover_error() -> CoverError {
        CoverError {
            source: image::load_from_memory(b"not an image").unwrap_err(),
        }
    }

    #[rstest]
    #[case::decoded_art(library::CoverArt::Image(Arc::new(RgbaImage::new(2, 2))), None)]
    #[case::no_art(library::CoverArt::Missing, None)]
    #[case::a_failed_decode(
        library::CoverArt::Error(broken_cover_error()),
        Some(Message::from(PaintEvent::Error(PaintError::Cover(
            Diagnostic::from_error(&broken_cover_error())
        ))))
    )]
    fn an_arriving_cover_raises_a_toast_only_when_the_decode_failed(
        #[case] art: library::CoverArt,
        #[case] expected: Option<Message>,
    ) {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());

        assert_eq!(painter.accept_cover(&decoded(art)), expected);
    }

    #[test]
    fn with_no_wash_staged_the_wash_starts_from_the_current_background() {
        let backdrop = test_backdrop(&CardCover::Missing, None);

        assert_eq!(backdrop.wash_from, backdrop.background);
    }

    #[test]
    fn a_staged_outgoing_background_is_where_the_wash_starts() {
        let outgoing = Color::Rgb(0x11, 0x22, 0x33);

        let backdrop = test_backdrop(&CardCover::Missing, Some(outgoing));

        assert_eq!(backdrop.wash_from, outgoing);
        assert_ne!(backdrop.wash_from, backdrop.background);
    }

    #[test]
    fn a_pixel_image_cover_stays_protected_from_effects() {
        let backdrop = test_backdrop(&CardCover::Image, None);

        assert_eq!(backdrop.layout.cover, Some(Rect::new(0, 0, 4, 4)));
    }

    #[test]
    fn a_text_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&CardCover::Text(Arc::default()), None);

        assert_eq!(backdrop.layout.cover, None);
    }

    #[test]
    fn a_missing_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&CardCover::Missing, None);

        assert_eq!(backdrop.layout.cover, None);
    }

    #[test]
    fn a_resize_reaches_the_painter_and_the_next_frame_uses_the_new_area() {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        let model = Model::default();
        let (_senders, latest, _doorbell) = runtime::latest_channels();
        let spectrum = SpectrumTap::silent();

        let reaction = painter.input(ShellInput::Terminal(Event::Resize(120, 40)));

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

    #[rstest]
    #[case::animations_off(Animations::Off)]
    #[case::animations_on(Animations::On)]
    fn commanded_window_colors_wait_for_their_own_theme(
        #[case] animations: Animations,
    ) {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        let commanded = ThemeName::from_static("ghost");

        painter.effect(
            ShellEffect::WindowColors(WindowColorsCmd::Set(commanded.clone())),
            animations,
        );

        assert_eq!(
            painter.window_colors_write,
            WindowColorsWrite::Staged(commanded)
        );
    }
}
