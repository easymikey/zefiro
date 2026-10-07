use std::{mem, sync::Arc};

use audio::spectrum::SpectrumAnalyzer;
use crossterm::event::Event;
use kernel::{
    cmd::{Playback, WindowColorsCmd},
    domain::{
        appearance::{Animations, Appearance},
        config::Diagnostic,
        cue::Cue,
        geometry::{Cells, Pixels},
    },
    message::PaintError,
    update::machine::{Machine, Unhandled},
};
use library::cover::CoverDecoded;
use ratatui::{Terminal, backend::Backend, layout::Rect};
use runtime::shell::{Frame, FrameDue, Painted, Reaction, ShellEffect};
use terminal::{
    capabilities::Capabilities,
    pixels::CoverPainter,
    window_colors::{reset_window_colors, write_window_colors},
};
use widgets::{
    animation::{
        stage::{AnimationStage, Backdrop, animation_frame_due},
        timings::TIMINGS,
    },
    card::{CardCover, clock_frame_due},
    pixels::cover::{
        CoverImage,
        CoverRefresh,
        CrossfadePermit,
        gate::{CoverArrival, CrossfadeGateMessage},
        pixmap::{CellPixels, cover_side},
        wash::cover_wash,
    },
    repaint::{Presence, progress_frame_due},
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
    window_colors::{WindowColorsMessage, WindowColorsWrite},
};

pub(crate) struct Painter<'terminal, B: Backend> {
    terminal: &'terminal mut Terminal<B>,
    presentation: ShellPresentation,
    cover_painter: CoverPainter,
    cell_pixels: CellPixels,
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
        let pixel_path = capabilities.pixel_path();
        let Capabilities {
            picker,
            color_depth,
        } = capabilities;
        let font_size = picker.font_size();
        let cell_aspect = terminal::capabilities::cell_aspect(font_size);
        let cell_pixels = CellPixels {
            width: Pixels(u32::from(font_size.width)),
            height: Pixels(u32::from(font_size.height)),
        };
        Self {
            terminal,
            presentation: ShellPresentation {
                cell_aspect,
                ..ShellPresentation::new(
                    presentation::theme(theme),
                    pixel_path,
                    color_depth,
                )
            },
            cover_painter: CoverPainter::new(picker, cell_pixels, pixel_path),
            cell_pixels,
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
            let message = CrossfadeGateMessage::TrackChanged(track);
            match self.motion.crossfade_gate.transition(message) {
                Ok(_) | Err(Unhandled) => {}
            }
        }
        self.presentation.key_hint_chords.follow(
            frame.model.workspace.keymap.bindings(),
            frame.model.revisions.config,
        );
        let latest_receivers = frame.latest_receivers;
        if let Some(theme) = latest_receivers.theme_receiver.take() {
            let outgoing_theme = ActiveTheme::new(
                &self.presentation.theme,
                self.presentation.color_depth,
            );
            self.motion.outgoing_theme_background =
                Some(BackdropStyle::from_theme(&outgoing_theme).background);
            self.presentation.theme = presentation::theme(Arc::unwrap_or_clone(theme));
        }
        if let Some(appearance) = latest_receivers.appearance_receiver.take() {
            self.presentation.appearance = *appearance;
        }
        let Some(cover) = latest_receivers.cover_receiver.take() else {
            return;
        };
        self.accept_cover(&cover);
    }

    fn flush_staged_window_colors(&mut self) {
        let wash = self.animation_stage.wash_progress();
        if wash.is_none() {
            self.motion.outgoing_theme_background = None;
        }
        let theme_name = self.presentation.theme.name.clone();
        self.drive_window_colors(WindowColorsMessage::Painted { theme_name, wash });
    }

    fn drive_window_colors(&mut self, message: WindowColorsMessage) {
        if let Ok(cmd) = self.window_colors_write.transition(message) {
            for write in cmd.into_parts().0 {
                self.set_window_colors(&write);
            }
        }
    }

    fn set_window_colors(&mut self, cmd: &WindowColorsCmd) {
        let written = match cmd {
            WindowColorsCmd::Set(_) => write_window_colors(&self.presentation.theme),
            WindowColorsCmd::Reset => reset_window_colors(),
        };
        if let Err(error) = written {
            self.errors
                .push(PaintError::WriteWindowColors(Diagnostic::from_error(
                    &error,
                )));
        }
    }

    fn accept_cover(&mut self, decoded: &CoverDecoded) {
        let CoverDecoded {
            path,
            cover_lookup,
            side: _side,
        } = decoded;
        let arrival = match cover_lookup {
            library::cover::CoverLookup::Found(image) => {
                self.cover_painter.set_cover(CoverImage {
                    path: path.clone(),
                    image: Arc::clone(image),
                });
                CoverArrival::Decoded
            }
            library::cover::CoverLookup::Missing => CoverArrival::Missing,
        };
        let message = CrossfadeGateMessage::CoverArrived {
            path: path.clone(),
            arrival,
        };
        match self.motion.crossfade_gate.transition(message) {
            Ok(_) | Err(Unhandled) => {}
        }
    }

    fn painted(&mut self, cover_area: Option<Rect>, visible_rows: Cells) -> Painted {
        Painted {
            cover_side: cover_area.map(|rect| cover_side(rect, self.cell_pixels)),
            visible_rows,
            errors: mem::take(&mut self.errors),
        }
    }

    fn backdrop<'a>(
        &self,
        animations: Animations,
        layout: FrameLayout<'a>,
    ) -> Backdrop<'a> {
        let theme =
            ActiveTheme::new(&self.presentation.theme, self.presentation.color_depth)
                .with_volume_pulse(TIMINGS.volume_pulse_mix);
        let style = BackdropStyle::from_theme(&theme);
        Backdrop {
            animations,
            layout,
            style,
            wash_from: self
                .motion
                .outgoing_theme_background
                .unwrap_or(style.background),
        }
    }

    fn smoothed_bands(&mut self, frame: &Frame<'_>) -> Spectrum {
        match self.motion.on_screen.spectrum {
            Presence::Shown => {
                let raw_bands = frame.model.player.is_playing().then(|| {
                    self.spectrum_analyzer
                        .bands::<SPECTRUM_BANDS>(frame.spectrum_tap)
                });
                let elapsed = frame.now.elapsed_since(self.motion.spectrum_advanced_at);
                self.motion.spectrum_advanced_at = frame.now;
                let feed = raw_bands
                    .as_ref()
                    .map_or(SpectrumFeed::Silent, SpectrumFeed::Live);
                self.motion.spectrum_smoothing.advance(feed, elapsed)
            }
            Presence::Hidden => *self.motion.spectrum_smoothing.bands(),
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

    fn input(&mut self, input: Self::Input) -> Reaction {
        if let ShellInput::Terminal(Event::Resize(width, height)) = &input {
            self.motion.area = Rect::new(0, 0, *width, *height);
            self.motion.screen_clear = ScreenClear::Due;
        }
        input::reaction_for(input)
    }

    fn effect(&mut self, effect: ShellEffect) {
        match effect {
            ShellEffect::WindowColors(cmd) => {
                self.drive_window_colors(WindowColorsMessage::Commanded(cmd));
            }
            ShellEffect::Animate(cue) => self.pending_cues.push(cue),
        }
    }

    fn frame_due(&self, frame: &Frame<'_>) -> FrameDue {
        let scene = view::scene(frame, &self.presentation, &self.motion);
        let next_frame = self.motion.next_frame();
        let animation = animation_frame_due(
            &self.animation_stage,
            self.cover_painter.motion(),
            next_frame,
        );
        let progress = progress_frame_due(
            scene.player,
            self.motion.on_screen.progress_bar_width,
            frame.now,
        );
        let clock =
            clock_frame_due(scene.player, self.motion.on_screen.clock, frame.now);
        let sleep = sleep_frame_due(
            scene.transport.sleep_timer.map(|timer| timer.deadline_at),
            self.motion.on_screen.sleep_label,
            frame.now,
        );
        let spectrum = match self.motion.on_screen.spectrum {
            Presence::Shown => self
                .motion
                .spectrum_smoothing
                .frame_due(Playback::from(scene.player), next_frame),
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
        self.motion.paint_clock = self.motion.paint_clock.record(frame.now);
        self.presentation.spectrum = self.smoothed_bands(&frame);
        let scene = view::scene(&frame, &self.presentation, &self.motion);
        let layout = FrameLayout::from_scene(&scene, self.motion.area);
        let crossfade_permit = match self
            .motion
            .crossfade_gate
            .transition(CrossfadeGateMessage::PermitTaken)
        {
            Ok(permit) => permit,
            Err(Unhandled) => CrossfadePermit::Withheld,
        };
        self.motion.on_screen = layout.on_screen(&scene);
        let wash = cover_wash(
            self.animation_stage.wash_progress(),
            Cells(self.motion.area.width),
        );
        let card_cover = self.cover_painter.refresh(
            &scene,
            CoverRefresh {
                cover_area: layout.cover_area,
                crossfade_permit,
                wash,
            },
        );
        let elapsed = self
            .animation_stage
            .advance_to(scene.presentation.since_first_paint);
        let backdrop = self.backdrop(
            scene.settings.appearance_settings.animations,
            protected_layout(&layout, &card_cover),
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
                &ScreenWidget::new(scene, &layout).card_cover(&card_cover),
                screen.area(),
            );
            pixels.paint(screen.buffer_mut(), &layout);
            animation_stage.play(cues, &backdrop);
            animation_stage.advance(screen.buffer_mut(), elapsed);
        })?;
        let (cover_area, visible_rows) =
            (layout.cover_area, layout.playlist_body_height());
        self.flush_staged_window_colors();
        Ok(self.painted(cover_area, visible_rows))
    }
}

fn protected_layout<'a>(
    layout: &FrameLayout<'a>,
    card_cover: &CardCover,
) -> FrameLayout<'a> {
    let cover_area = if matches!(card_cover, CardCover::Image) {
        layout.cover_area
    } else {
        None
    };
    FrameLayout {
        cover_area,
        remaining_label: String::new(),
        overlay_content: None,
        ..*layout
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use audio::tap::SpectrumTap;
    use crossterm::event::Event;
    use kernel::domain::{
        appearance::Animations,
        geometry::Cells,
        model::Model,
        time::Moment,
    };
    use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
    use rstest::rstest;
    use runtime::{
        repaint::FRAME_INTERVAL,
        shell::{Frame, FrameDue, Reaction, Shell as _},
    };
    use terminal::capabilities::{Capabilities, TerminalEnvironment};
    use widgets::{
        animation::stage::Backdrop,
        card::CardCover,
        repaint::Presence,
        scene::PixelPath,
        screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
        spectrum::{SPECTRUM_BANDS, SpectrumFeed},
        theme::rgb::ColorDepth,
    };

    use crate::{
        shell::{
            motion::{PaintClock, ScreenClear},
            painter::{Painter, protected_layout},
            presentation::{ShellPresentation, theme},
            shell_input::ShellInput,
            view,
            window_colors::WindowColorsWrite,
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

    fn test_backdrop(
        card_cover: &CardCover,
        outgoing: Option<Color>,
    ) -> Backdrop<'static> {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        painter.presentation = ShellPresentation::new(
            theme(fallback_theme()),
            PixelPath::Halfblocks,
            ColorDepth::TrueColor,
        );
        painter.motion.outgoing_theme_background = outgoing;
        let layout = FrameLayout {
            screen: Rect::new(0, 0, 40, 10),
            breakpoint: Breakpoint::Full,
            content: Rect::default(),
            header: Rect::default(),
            card_metrics: None,
            progress_bar_width: Cells(0),
            remaining_label: String::new(),
            cover_area: Some(Rect::new(0, 0, 4, 4)),
            playlist_pane: Rect::default(),
            playlist_areas: None,
            key_hints: None,
            search_bounds: Rect::default(),
            overlay_areas: None,
            overlay_content: None,
            toast: Some(Rect::new(0, 0, 10, 1)),
        };
        painter.backdrop(Animations::On, protected_layout(&layout, card_cover))
    }

    #[test]
    fn with_no_wash_staged_the_wash_starts_from_the_current_background() {
        let backdrop = test_backdrop(&CardCover::Missing, None);

        assert_eq!(backdrop.wash_from, backdrop.style.background);
    }

    #[test]
    fn a_staged_outgoing_background_is_where_the_wash_starts() {
        let outgoing = Color::Rgb(0x11, 0x22, 0x33);

        let backdrop = test_backdrop(&CardCover::Missing, Some(outgoing));

        assert_eq!(backdrop.wash_from, outgoing);
        assert_ne!(backdrop.wash_from, backdrop.style.background);
    }

    #[test]
    fn a_pixel_image_cover_stays_protected_from_effects() {
        let backdrop = test_backdrop(&CardCover::Image, None);

        assert_eq!(backdrop.layout.cover_area, Some(Rect::new(0, 0, 4, 4)));
    }

    #[test]
    fn a_text_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&CardCover::Text(Arc::default()), None);

        assert_eq!(backdrop.layout.cover_area, None);
    }

    #[test]
    fn a_missing_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&CardCover::Missing, None);

        assert_eq!(backdrop.layout.cover_area, None);
    }

    #[test]
    fn a_resize_reaches_the_painter_and_the_next_frame_uses_the_new_area() {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        let model = Model::default();
        let (_senders, latest_receivers, _doorbell) =
            runtime::latest::latest_channels();
        let spectrum = SpectrumTap::silent();

        let reaction = painter.input(ShellInput::Terminal(Event::Resize(120, 40)));

        let frame = Frame {
            model: &model,
            spectrum_tap: &spectrum,
            latest_receivers: &latest_receivers,
            now: Moment::new(Duration::from_secs(5)),
        };
        let scene = view::scene(&frame, &painter.presentation, &painter.motion);
        let layout = FrameLayout::from_scene(&scene, painter.motion.area);
        assert_eq!(reaction, Reaction::Repaint);
        assert_eq!(layout.screen, Rect::new(0, 0, 120, 40));
        assert_eq!(painter.motion.screen_clear, ScreenClear::Due);
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
        painter.motion.paint_clock = PaintClock::Painted {
            first: paint_time(),
            last: paint_time(),
        };
        let lifted = painter.motion.spectrum_smoothing.advance(
            SpectrumFeed::Live(&[1.0; SPECTRUM_BANDS]),
            Duration::from_secs(1),
        );
        assert!(lifted.iter().all(|&band| band > 0.0));
        let model = Model::default();
        let (_senders, latest_receivers, _doorbell) =
            runtime::latest::latest_channels();
        let silent = SpectrumTap::silent();
        let frame = Frame {
            model: &model,
            spectrum_tap: &silent,
            latest_receivers: &latest_receivers,
            now: paint_time(),
        };

        assert_eq!(painter.frame_due(&frame), expected);
    }

    #[test]
    fn animations_off_writes_the_window_colors_once() {
        let mut terminal = test_terminal();
        let mut painter =
            Painter::new(&mut terminal, test_theme(), test_capabilities());
        let mut model = Model::default();
        model.settings.appearance_settings.animations = Animations::Off;
        let (_senders, latest_receivers, _doorbell) =
            runtime::latest::latest_channels();
        let spectrum = SpectrumTap::silent();
        painter.window_colors_write =
            WindowColorsWrite::Staged(painter.presentation.theme.name.clone());
        let frame = Frame {
            model: &model,
            spectrum_tap: &spectrum,
            latest_receivers: &latest_receivers,
            now: paint_time(),
        };

        let painted = painter.paint(frame).unwrap();

        assert_eq!(painter.window_colors_write, WindowColorsWrite::Done);
        assert!(painted.errors.is_empty());
    }
}
