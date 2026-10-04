use std::{mem, sync::Arc, time::Duration};

use audio::spectrum::SpectrumAnalyzer;
use crossterm::event::Event;
use kernel::{
    cmd::WindowColorsCmd,
    domain::{
        appearance::Animations,
        config::Diagnostic,
        cue::Cue,
        geometry::{Cells, Pixels},
        theme::ThemeName,
        time::Moment,
    },
    message::PaintError,
};
use library::cover::CoverDecoded;
use ratatui::{Terminal, backend::Backend, layout::Rect};
use runtime::shell::{Frame, FrameDue, Painted, Reaction, ShellEffect};
use terminal::{
    capabilities::Capabilities,
    pixels::CoverPainter,
    window_colors::write_window_colors,
};
use widgets::{
    animation::stage::{AnimationStage, Backdrop, animation_frame_due},
    appearance::Appearance,
    card::{CardCover, clock_frame_due},
    key_hints::KeyHintChords,
    pixels::cover::{
        CoverImage,
        CoverMotion,
        CoverRefresh,
        gate::{CoverArrival, CrossfadeGateMessage},
        pixmap::CellPixels,
        wash::cover_wash,
    },
    primitive::bar::progress_frame_due,
    repaint::Presence,
    screen::{frame_layout::FrameLayout, root::ScreenWidget},
    spectrum::{SPECTRUM_BANDS, Spectrum, SpectrumFeed},
    status_line::sleep_frame_due,
    theme::{active_theme::ActiveTheme, backdrop_style::BackdropStyle},
};

use crate::shell::{
    input,
    motion::{Motion, ScreenClear},
    presentation::{self, ShellPresentation},
    shell_input::ShellInput,
    view,
};

const NO_BANDS: Spectrum = [0.0; SPECTRUM_BANDS];

#[derive(Debug, Clone, PartialEq, Eq)]
enum WindowColorsWrite {
    Done,
    Staged(ThemeName),
}

pub(crate) struct Painter<'terminal, B: Backend> {
    terminal: &'terminal mut Terminal<B>,
    presentation: ShellPresentation,
    cover_painter: CoverPainter,
    cell: CellPixels,
    spectrum_analyzer: SpectrumAnalyzer,
    motion: Motion,
    pending_cues: Vec<Cue>,
    animation_stage: AnimationStage,
    window_colors_write: WindowColorsWrite,
    errors: Vec<PaintError>,
}

impl<B: Backend> std::fmt::Debug for Painter<'_, B> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Painter").finish_non_exhaustive()
    }
}

impl<'terminal, B: Backend> Painter<'terminal, B> {
    pub(crate) fn new(
        terminal: &'terminal mut Terminal<B>,
        theme: config::theme_file::TomlTheme,
        capabilities: Capabilities,
    ) -> Self {
        let area = terminal.get_frame().area();
        let Capabilities {
            picker,
            pixel_path,
            color_depth,
        } = capabilities;
        let font_size = picker.font_size();
        let cell_aspect = terminal::capabilities::cell_aspect(font_size);
        Self {
            terminal,
            presentation: ShellPresentation {
                theme: presentation::theme(theme),
                appearance: Appearance::default(),
                pixel_path,
                color_depth,
                cell_aspect,
                home: dirs::home_dir(),
                spectrum: [0.0; SPECTRUM_BANDS],
                key_hint_chords: KeyHintChords::default(),
            },
            cover_painter: CoverPainter::new(picker),
            cell: CellPixels {
                width: Pixels(u32::from(font_size.width)),
                height: Pixels(u32::from(font_size.height)),
            },
            spectrum_analyzer: SpectrumAnalyzer::new(),
            motion: Motion {
                area,
                ..Motion::default()
            },
            pending_cues: Vec::new(),
            animation_stage: AnimationStage::default(),
            window_colors_write: WindowColorsWrite::Done,
            errors: Vec::new(),
        }
    }

    pub(crate) fn with_appearance(self, appearance: Appearance) -> Self {
        Self {
            presentation: ShellPresentation {
                appearance,
                ..self.presentation
            },
            ..self
        }
    }

    fn take_latest(&mut self, frame: &Frame<'_>) {
        if self.pending_cues.contains(&Cue::TrackChanged) {
            let track = frame
                .model
                .player
                .current()
                .map(|track| track.path().to_path_buf());
            self.motion
                .crossfade_gate
                .settle(CrossfadeGateMessage::TrackChanged(track));
        }
        self.presentation.key_hint_chords.follow(
            frame.model.workspace.keymap.bindings(),
            frame.model.revisions.config,
        );
        let latest = frame.latest;
        if let Some(theme) = latest.theme.take() {
            let outgoing = ActiveTheme::new(
                &self.presentation.theme,
                self.presentation.color_depth,
            );
            self.motion.outgoing_theme_background =
                Some(BackdropStyle::from_theme(&outgoing).background);
            self.presentation.theme = presentation::theme(Arc::unwrap_or_clone(theme));
        }
        if let Some(appearance) = latest.appearance.take() {
            self.presentation.appearance = presentation::appearance(&appearance);
        }
        let Some(cover) = latest.cover.take() else {
            return;
        };
        self.accept_cover(&cover);
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
            self.errors
                .push(PaintError::WindowColors(Diagnostic::from_error(&error)));
        }
    }

    fn accept_cover(&mut self, decoded: &CoverDecoded) {
        let CoverDecoded { path, art, .. } = decoded;
        let arrival = match art {
            library::cover::CoverArt::Image(image) => {
                self.cover_painter.set_cover(CoverImage {
                    path: path.clone(),
                    image: Arc::clone(image),
                });
                CoverArrival::Decoded
            }
            library::cover::CoverArt::Missing => CoverArrival::Missing,
        };
        self.motion.crossfade_gate.cover_arrived(path, arrival);
    }

    fn backdrop(&self, animations: Animations, layout: FrameLayout) -> Backdrop {
        let theme =
            ActiveTheme::new(&self.presentation.theme, self.presentation.color_depth)
                .with_volume_pulse(self.animation_stage.timings().volume_pulse_mix);
        let style = BackdropStyle::from_theme(&theme);
        Backdrop {
            animations,
            layout,
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

    fn cover_motion(&self, now: Moment) -> CoverMotion {
        let elapsed = self
            .motion
            .first_paint
            .map_or(Duration::ZERO, |first| now.elapsed_since(first));
        self.cover_painter.cover_motion(elapsed)
    }

    fn smoothed_bands(&mut self, frame: &Frame<'_>) -> Spectrum {
        match self.motion.on_screen.spectrum {
            Presence::Shown => {
                let raw_bands = self.raw_bands(frame);
                let elapsed = frame.now.elapsed_since(self.motion.spectrum_advanced_at);
                self.motion.spectrum_advanced_at = frame.now;
                let feed = SpectrumFeed::of(&frame.model.player, &raw_bands);
                self.motion.spectrum_smoothing.advance(feed, elapsed)
            }
            Presence::Hidden => *self.motion.spectrum_smoothing.bands(),
        }
    }

    fn raw_bands(&mut self, frame: &Frame<'_>) -> Spectrum {
        match (
            self.motion.on_screen.spectrum,
            frame.model.player.is_playing(),
        ) {
            (Presence::Shown, true) => self
                .spectrum_analyzer
                .bands::<SPECTRUM_BANDS>(frame.spectrum),
            (Presence::Shown | Presence::Hidden, _) => NO_BANDS,
        }
    }
}

impl<B> runtime::shell::Shell for Painter<'_, B>
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

    fn effect(&mut self, effect: ShellEffect) {
        match effect {
            ShellEffect::WindowColors(WindowColorsCmd::Set(name)) => {
                self.window_colors_write = WindowColorsWrite::Staged(name);
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
        let next_frame = self.motion.next_frame();
        let animation = animation_frame_due(
            &self.animation_stage,
            self.cover_motion(frame.now),
            next_frame,
        );
        let progress = progress_frame_due(
            scene.player,
            self.motion.on_screen.progress_bar,
            frame.now,
        );
        let clock =
            clock_frame_due(scene.player, self.motion.on_screen.clock, frame.now);
        let sleep = sleep_frame_due(
            scene.transport.sleep.map(|timer| timer.deadline),
            self.motion.on_screen.sleep_label,
            frame.now,
        );
        let spectrum = match self.motion.on_screen.spectrum {
            Presence::Shown => self
                .motion
                .spectrum_smoothing
                .frame_due(SpectrumFeed::of(scene.player, &NO_BANDS), next_frame),
            Presence::Hidden => None,
        };
        [animation, progress, clock, sleep, spectrum]
            .into_iter()
            .flatten()
            .min()
            .map_or(FrameDue::Settled, FrameDue::At)
    }

    fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Self::Error> {
        self.take_latest(&frame);
        match frame.model.settings.appearance.animations {
            Animations::Off => self.write_staged_window_colors(),
            Animations::On => {}
        }
        self.motion.record_first_paint(frame.now);
        self.motion.last_paint = frame.now;
        self.presentation.spectrum = self.smoothed_bands(&frame);
        let scene = view::scene(&frame, &self.presentation, &self.motion);
        let layout = FrameLayout::from_scene(&scene, self.motion.area);
        let crossfade = self.motion.crossfade_gate.permit();
        self.motion.on_screen = layout.on_screen(&scene);
        let wash = cover_wash(
            self.animation_stage.wash_progress(),
            Cells(self.motion.area.width),
        );
        let cover_art = self.cover_painter.refresh(
            &scene,
            CoverRefresh {
                cover: layout.cover,
                crossfade,
                wash,
            },
        );
        let elapsed = self.animation_stage.advance_clock(scene.clock);
        let backdrop = self.backdrop(
            scene.appearance_settings().animations,
            protected_layout(layout, &cover_art),
        );
        if mem::replace(&mut self.motion.screen_clear, ScreenClear::NotDue)
            == ScreenClear::Due
        {
            self.terminal.clear()?;
        }
        let (pixels, animation_stage) =
            (&mut self.cover_painter, &mut self.animation_stage);
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
            cover_side: layout.cover.map(|rect| cover_side(rect, self.cell)),
            visible_rows: layout.playlist_body_height(),
            errors: mem::take(&mut self.errors),
        })
    }
}

fn cover_side(rect: Rect, cell: CellPixels) -> Pixels {
    Pixels(u32::from(rect.height).saturating_mul(cell.height.0))
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
    use std::{sync::Arc, time::Duration};

    use audio::tap::SpectrumTap;
    use crossterm::event::Event;
    use kernel::{
        cmd::WindowColorsCmd,
        domain::{
            appearance::Animations,
            model::Model,
            theme::ThemeName,
            time::Moment,
        },
    };
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
    use rstest::rstest;
    use runtime::{
        repaint::FRAME_INTERVAL,
        shell::{Frame, FrameDue, Reaction, Shell as _, ShellEffect},
    };
    use terminal::capabilities::{Capabilities, TerminalEnvironment};
    use widgets::{
        animation::stage::Backdrop,
        card::CardCover,
        repaint::Presence,
        screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
        spectrum::SPECTRUM_BANDS,
    };

    use crate::{
        shell::{
            motion::ScreenClear,
            painter::{Painter, WindowColorsWrite, protected_layout},
            presentation::test_presentation,
            shell_input::ShellInput,
            view,
        },
        startup::fallback_theme,
    };

    fn test_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn test_capabilities() -> Capabilities {
        Capabilities::from_environment(&TerminalEnvironment::default())
    }

    fn test_theme() -> config::theme_file::TomlTheme {
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
            toast: Some(Rect::new(0, 0, 10, 1)),
        };
        painter.backdrop(Animations::On, protected_layout(layout, cover_art))
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
        let (_senders, latest, _doorbell) = runtime::latest::latest_channels();
        let spectrum = SpectrumTap::silent();

        let reaction = painter.input(ShellInput::Terminal(Event::Resize(120, 40)));

        let frame = Frame {
            model: &model,
            spectrum: &spectrum,
            latest: &latest,
            now: Moment::new(Duration::from_secs(5)),
        };
        let scene = view::scene(&frame, &painter.presentation, &painter.motion);
        let layout = FrameLayout::from_scene(&scene, painter.motion.area);
        assert_eq!(reaction, Reaction::Repaint);
        assert_eq!(layout.screen, Rect::new(0, 0, 120, 40));
        assert_eq!(painter.motion.screen_clear, ScreenClear::Due);
    }

    #[test]
    fn commanded_window_colors_wait_for_their_own_theme() {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        let commanded = ThemeName::from_static("ghost");

        painter.effect(ShellEffect::WindowColors(WindowColorsCmd::Set(
            commanded.clone(),
        )));

        assert_eq!(
            painter.window_colors_write,
            WindowColorsWrite::Staged(commanded)
        );
    }

    fn paint_time() -> Moment {
        Moment::new(Duration::from_secs(10))
    }

    #[rstest]
    #[case::shown_while_bands_can_move(
        Presence::Shown,
        FrameDue::At(Moment::new(paint_time().since_epoch() + FRAME_INTERVAL))
    )]
    #[case::hidden(Presence::Hidden, FrameDue::Settled)]
    fn a_spectrum_frame_is_due_only_while_the_spectrum_is_on_screen(
        #[case] spectrum: Presence,
        #[case] expected: FrameDue,
    ) {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        painter.motion.on_screen.spectrum = spectrum;
        painter.motion.last_paint = paint_time();
        let lifted = painter
            .motion
            .spectrum_smoothing
            .smooth(&[1.0; SPECTRUM_BANDS], Duration::from_secs(1));
        assert!(lifted.iter().all(|&band| band > 0.0));
        let model = Model::default();
        let (_senders, latest, _doorbell) = runtime::latest::latest_channels();
        let silent = SpectrumTap::silent();
        let frame = Frame {
            model: &model,
            spectrum: &silent,
            latest: &latest,
            now: paint_time(),
        };

        assert_eq!(painter.frame_due(&frame), expected);
    }
}
