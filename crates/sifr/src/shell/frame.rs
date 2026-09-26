use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use audio::SpectrumAnalyzer;
use config::{AppearanceFile, AppearancePatch, Hex, ThemeColors, ThemeFile};
use kernel::{Cue, WindowColorsCmd, domain::ThemeName};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Rect, Size},
    style::Color,
};
use runtime::{CoverDecoded, FrameDue, Painted, Reload, ShellEffect, View};
use terminal::{
    Capabilities,
    CoverArtOwner,
    CoverFade,
    CoverMotion,
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
    CellAspect,
    ColorDepth,
    FrameLayout,
    PixelPath,
    PlaylistAreas,
    SPECTRUM_BANDS,
    Scene,
    Screen,
    SpectrumSmoothing,
    Theme,
    abbreviate_home,
};

use crate::{
    shell::{
        backdrop::{BackdropSources, animation_backdrop},
        cover_fade::{
            CoverArrival,
            CoverFadePermission,
            CoverPlacement,
            CoverWanted,
            cover_arrived_fade,
            cover_outcome,
            cover_sources,
            cover_wash,
            desired_cover,
            resolved_cover_fade,
            track_changed_fade,
        },
        frame_clock::{
            animation_frame_due,
            earliest_frame_due,
            frame_effect,
            playhead_frame_due,
        },
        reload,
        window_colors::{
            PendingWindowColors,
            WindowColorsPlan,
            settle_window_colors_plan,
            window_colors_plan,
        },
    },
    toast::ShellFailure,
};

pub(crate) struct Frame {
    theme: Theme,
    appearance: AppearanceFile,
    home: Option<PathBuf>,
    music_dir: PathBuf,
    music_dir_display: String,
    pixel_path: PixelPath,
    color_depth: ColorDepth,
    cell_aspect: CellAspect,
    pixels: Pixels,
    spectrum_analyzer: SpectrumAnalyzer,
    spectrum_smoothing: SpectrumSmoothing,
    spectrum_at: Instant,
    started: Instant,
    last_paint: Instant,
    playlist_body_height: u16,
    wanted_cover: Option<PathBuf>,
    pending_cues: Vec<Cue>,
    animation_stage: AnimationStage,
    motion: FrameDue,
    resize: ResizeState,
    cover_fade: CoverFadePermission,
    outgoing_theme_background: Option<Color>,
    pending_window_colors: PendingWindowColors,
    settled_failure: Option<ShellFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeState {
    Clean,
    Resized,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Frame").finish_non_exhaustive()
    }
}

impl Frame {
    pub(crate) fn new() -> Self {
        let Capabilities {
            picker,
            pixel_path,
            color_depth,
        } = terminal::resolve_immediate(&TerminalEnvironment::current());
        let cell_aspect = terminal::cell_aspect(picker.font_size());
        Self {
            theme: initial_theme(),
            appearance: AppearanceFile::default(),
            home: dirs::home_dir(),
            music_dir: PathBuf::new(),
            music_dir_display: String::new(),
            pixel_path,
            color_depth,
            cell_aspect,
            pixels: Pixels::new(picker),
            spectrum_analyzer: SpectrumAnalyzer::new(),
            spectrum_smoothing: SpectrumSmoothing::default(),
            spectrum_at: Instant::now(),
            started: Instant::now(),
            last_paint: Instant::now(),
            playlist_body_height: 0,
            wanted_cover: None,
            pending_cues: Vec::new(),
            animation_stage: AnimationStage::default(),
            motion: FrameDue::Settled,
            resize: ResizeState::Clean,
            cover_fade: CoverFadePermission::Absent,
            outgoing_theme_background: None,
            pending_window_colors: PendingWindowColors::Idle,
            settled_failure: None,
        }
    }

    pub(crate) fn adopt(&mut self, answer: ProbeAnswer) {
        self.pixel_path = answer.pixel_path;
        self.cell_aspect = terminal::cell_aspect(answer.picker.font_size());
        self.pixels.adopt(answer.picker);
    }

    pub(crate) fn mark_resized(&mut self) {
        self.resize = ResizeState::Resized;
    }

    pub(crate) fn reloaded(&mut self, reload: Reload) -> Option<ShellFailure> {
        let is_theme_reload = matches!(reload, Reload::Theme(_));
        if is_theme_reload {
            self.outgoing_theme_background =
                Some(ActiveTheme::new(&self.theme, self.color_depth).window_bg());
        }
        reload::install(&mut self.theme, &mut self.appearance, reload);
        None
    }

    pub(crate) fn patched(&mut self, patch: AppearancePatch) -> Option<ShellFailure> {
        reload::apply_patch(&mut self.appearance, patch);
        None
    }

    fn stage_window_colors(&mut self) -> Option<ShellFailure> {
        match window_colors_plan(self.appearance.window.animations) {
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
            self.outgoing_theme_background = None;
        }
        if settle_window_colors_plan(self.pending_window_colors, progress) {
            self.pending_window_colors = PendingWindowColors::Idle;
            self.settled_failure = self.apply_window_colors();
        }
    }

    pub(crate) fn take_settled_failure(&mut self) -> Option<ShellFailure> {
        std::mem::take(&mut self.settled_failure)
    }

    fn apply_window_colors(&self) -> Option<ShellFailure> {
        ThemeName::new(self.theme.name.clone()).map_or_else(
            |_| {
                Some(ShellFailure::Theme(UnknownThemeError {
                    name: self.theme.name.clone(),
                }))
            },
            |name| {
                write_window_colors(&WindowColorsCmd::Apply(name), &self.theme)
                    .err()
                    .map(ShellFailure::Theme)
            },
        )
    }

    pub(crate) fn effect(&mut self, effect: &ShellEffect) -> Option<ShellFailure> {
        match effect {
            ShellEffect::Appearance(patch) => self.patched(*patch),
            ShellEffect::WindowColors(WindowColorsCmd::Apply(_)) => {
                self.stage_window_colors()
            }
            ShellEffect::WindowColors(WindowColorsCmd::Reset) => {
                write_window_colors(&WindowColorsCmd::Reset, &self.theme)
                    .err()
                    .map(ShellFailure::Theme)
            }
            ShellEffect::Animate(cue) => {
                self.pending_cues.push(*cue);
                None
            }
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

    fn cover_fade(&mut self, current_track: Option<&Path>) -> CoverFade {
        self.note_track_change(current_track);
        self.take_cover_fade()
    }

    fn backdrop(
        &self,
        layout: FrameLayout,
        cover_art_owner: &CoverArtOwner,
    ) -> Backdrop {
        animation_backdrop(
            &BackdropSources {
                theme: &self.theme,
                color_depth: self.color_depth,
                appearance: &self.appearance,
                mix: self.animation_stage.timings().volume_pulse_mix,
                outgoing_background: self.outgoing_theme_background,
            },
            layout,
            cover_art_owner,
        )
    }

    fn note_track_change(&mut self, current_track: Option<&Path>) {
        let current = std::mem::take(&mut self.cover_fade);
        self.cover_fade =
            track_changed_fade(current, &self.pending_cues, current_track);
    }

    fn note_cover_arrived(&mut self, path: &Path, arrival: CoverArrival) {
        let current = std::mem::take(&mut self.cover_fade);
        self.cover_fade = cover_arrived_fade(current, path, arrival);
    }

    fn take_cover_fade(&mut self) -> CoverFade {
        let current = std::mem::take(&mut self.cover_fade);
        let (next, fade) = resolved_cover_fade(current);
        self.cover_fade = next;
        fade
    }

    pub(crate) fn frame_due(&self) -> FrameDue {
        let effect = frame_effect(&self.animation_stage, self.cover_motion());
        let animation = animation_frame_due(effect, self.last_paint);
        earliest_frame_due(animation, self.motion)
    }

    fn cover_motion(&self) -> CoverMotion {
        self.pixels.cover_motion(self.started.elapsed())
    }

    fn note_motion(&mut self, view: &View<'_>) {
        self.motion = playhead_frame_due(&view.model.player, view.now, self.last_paint);
    }

    fn refresh_music_dir(&mut self, music_dir: &Path) {
        if music_dir == self.music_dir {
            return;
        }
        self.music_dir = music_dir.to_path_buf();
        self.music_dir_display = self.home.as_deref().map_or_else(
            || music_dir.display().to_string(),
            |home| abbreviate_home(music_dir, home),
        );
    }

    fn step_spectrum(&mut self, raw: &widgets::Spectrum) -> widgets::Spectrum {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.spectrum_at);
        self.spectrum_at = now;
        self.spectrum_smoothing.smooth(raw, elapsed)
    }

    pub(crate) fn paint(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        view: View<'_>,
    ) -> Result<Painted, io::Error> {
        self.last_paint = Instant::now();
        self.note_motion(&view);
        self.refresh_music_dir(view.model.music_dir.as_path());
        let current_track = view.model.player.current().map(|track| track.path());
        let fade = self.cover_fade(current_track);
        let raw_bands = self
            .spectrum_analyzer
            .bands::<SPECTRUM_BANDS>(view.spectrum);
        let bands = self.step_spectrum(&raw_bands);
        let scene = build_scene(
            &SceneSources {
                theme: &self.theme,
                color_depth: self.color_depth,
                appearance: &self.appearance,
                pixel_path: self.pixel_path,
                cell_aspect: self.cell_aspect,
                started: self.started,
                music_dir_display: &self.music_dir_display,
            },
            &view,
            &bands,
        );
        let cover = desired_cover(
            &mut self.wanted_cover,
            &CoverWanted {
                current: current_track,
                style: scene.cover_style(),
                side: self.appearance.cover.size_px,
            },
        );
        let area = terminal_area(terminal.size()?);
        let layout = FrameLayout::new(&scene.layout_inputs(), area);
        let viewport =
            note_playlist_height(&mut self.playlist_body_height, layout.playlist);
        let wash = cover_wash(self.animation_stage.wash_progress(), area.width);
        let cover_art_owner = self
            .pixels
            .refresh(cover_sources(scene, CoverPlacement { layout, fade, wash }));
        let cover_art = cover_art_owner.as_cover_art();
        let elapsed = self.animation_stage.elapsed_since(scene.clock);
        let backdrop = self.backdrop(layout, &cover_art_owner);
        clear_if_resized(&mut self.resize, terminal)?;
        let (pixels, animation_stage) = (&mut self.pixels, &mut self.animation_stage);
        let cues = std::mem::take(&mut self.pending_cues);
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
        Ok(Painted { cover, viewport })
    }
}

struct SceneSources<'a> {
    theme: &'a Theme,
    color_depth: ColorDepth,
    appearance: &'a AppearanceFile,
    pixel_path: PixelPath,
    cell_aspect: CellAspect,
    started: Instant,
    music_dir_display: &'a str,
}

fn build_scene<'a>(
    sources: &SceneSources<'a>,
    view: &View<'a>,
    bands: &'a widgets::Spectrum,
) -> Scene<'a> {
    Scene {
        model: view.model,
        theme: sources.theme,
        color_depth: sources.color_depth,
        appearance: sources.appearance,
        bindings: view.model.workspace.bindings.as_slice(),
        spectrum: bands,
        pixel_path: sources.pixel_path,
        cell_aspect: sources.cell_aspect,
        clock: sources.started.elapsed(),
        now_unix: now_unix(),
        now: view.now,
        music_dir: sources.music_dir_display,
        sleep_left: sleep_left(view.sleep_deadline, Instant::now()),
    }
}

fn terminal_area(size: Size) -> Rect {
    Rect::new(0, 0, size.width, size.height)
}

fn clear_if_resized(
    resize: &mut ResizeState,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), io::Error> {
    if let ResizeState::Resized = std::mem::replace(resize, ResizeState::Clean) {
        terminal.clear()?;
    }
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn playlist_body_height(playlist: Option<PlaylistAreas>) -> u16 {
    playlist.map_or(0, |areas| areas.body.height)
}

fn note_playlist_height(
    tracked: &mut u16,
    playlist: Option<PlaylistAreas>,
) -> Option<usize> {
    let previous = *tracked;
    *tracked = playlist_body_height(playlist);
    (*tracked != previous).then_some(usize::from(*tracked))
}

fn sleep_left(sleep_deadline: Option<Instant>, now: Instant) -> Option<Duration> {
    sleep_deadline.map(|deadline| deadline.saturating_duration_since(now))
}

fn initial_theme() -> Theme {
    config::embedded_theme("noir")
        .and_then(|source| config::parse_theme(source, "noir").ok())
        .map_or_else(fallback_theme, Theme::from)
}

pub(crate) fn fallback_theme() -> Theme {
    Theme::from(ThemeFile {
        name: "fallback".to_string(),
        colors: ThemeColors {
            background: Hex([0, 0, 0]),
            foreground: Hex([0xff, 0xff, 0xff]),
            bright_foreground: Hex([0xff, 0xff, 0xff]),
            accent: Hex([0xff, 0xff, 0xff]),
            green: Hex([0, 0xff, 0]),
            yellow: Hex([0xff, 0xff, 0]),
            red: Hex([0xff, 0, 0]),
            window_background: None,
        },
        scanning_label: "scanning…".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ratatui::layout::Rect;
    use widgets::PlaylistAreas;

    use crate::shell::frame::{playlist_body_height, sleep_left};

    #[test]
    fn no_deadline_has_no_sleep_left() {
        let now = Instant::now();

        assert_eq!(sleep_left(None, now), None);
    }

    #[test]
    fn a_future_deadline_counts_down_to_it() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(90);

        assert_eq!(
            sleep_left(Some(deadline), now),
            Some(Duration::from_secs(90))
        );
    }

    #[test]
    fn a_past_deadline_has_no_time_left() {
        let now = Instant::now();
        let deadline = now - Duration::from_secs(1);

        assert_eq!(sleep_left(Some(deadline), now), Some(Duration::ZERO));
    }

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
}
