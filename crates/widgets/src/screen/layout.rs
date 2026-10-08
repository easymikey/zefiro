use kernel::domain::{
    appearance::{CoverMode, KeyHints, ProgressTime},
    geometry::Cells,
    overlay::Overlay,
};
use ratatui::layout::{Constraint, Layout, Rect};

use crate::{
    card::{
        self,
        CardView,
        compact::{compact_height, progress_bar_width as compact_progress_bar_width},
        metrics::CardMetrics,
    },
    overlay::layer::{OverlayView, OverlayWidget},
    playlist::{pane::PlaylistWidget, row::PlaylistAreas, view::PlaylistView},
    primitive::bar::remaining_label,
    repaint::{OnScreen, Presence},
    scene::Scene,
    screen::{
        breakpoint::Breakpoint,
        frame_layout::FrameLayout,
        minimal::progress_bar_width as minimal_progress_bar_width,
    },
    toast::ToastWidget,
};

const MAX_WIDTH: u16 = 100;
const MARGIN: u16 = 1;
const KEY_HINTS_ROWS: u16 = 1;

impl<'a> FrameLayout<'a> {
    #[must_use]
    pub fn from_scene(scene: &Scene<'a>, screen: Rect) -> Self {
        let body = body(scene, screen);
        if body.breakpoint == Breakpoint::TooSmall {
            return body;
        }
        let overlay_view = OverlayView::from_scene(scene);
        let body = Self {
            overlay_content: overlay_view.content(screen),
            ..body
        };
        let remaining_label = match scene.settings.appearance_settings.progress_time {
            ProgressTime::Remaining if body.breakpoint == Breakpoint::Full => {
                remaining_label(CardView::from_scene(scene).remaining())
            }
            ProgressTime::Remaining | ProgressTime::Elapsed => String::new(),
        };
        Self {
            progress_bar_width: body.progress_bar_width(scene, &remaining_label),
            remaining_label,
            overlay_areas: OverlayWidget::new(overlay_view, &body)
                .avoid(body.cover_exclusion(scene.cover_mode()))
                .areas(screen),
            toast_placement: ToastWidget::from_scene(scene)
                .and_then(|toast_widget| toast_widget.area(screen, body.breakpoint)),
            ..body
        }
    }

    #[must_use]
    pub fn on_screen(&self, scene: &Scene<'_>) -> OnScreen {
        OnScreen {
            progress_bar_width: self.is_card_shown().then_some(self.progress_bar_width),
            clock: Presence::from(self.is_card_shown()),
            sleep_label: Presence::from(
                self.playlist_areas.is_some() && scene.transport.sleep_timer.is_some(),
            ),
            spectrum: Presence::from(self.is_spectrum_shown(scene)),
        }
    }

    fn is_card_shown(&self) -> bool {
        match self.breakpoint {
            Breakpoint::Full => self.card_metrics.is_some(),
            Breakpoint::Compact | Breakpoint::Minimal => true,
            Breakpoint::TooSmall => false,
        }
    }

    fn is_spectrum_shown(&self, scene: &Scene<'_>) -> bool {
        let card_spectrum = self
            .card_metrics
            .is_some_and(|metrics| !metrics.spectrum_row.is_empty());
        let milkdrop_spectrum =
            scene.cover_mode() == CoverMode::Milkdrop && self.cover_area.is_some();
        card_spectrum || milkdrop_spectrum
    }

    fn progress_bar_width(&self, scene: &Scene<'_>, remaining_label: &str) -> Cells {
        match self.breakpoint {
            Breakpoint::Full => self.card_metrics.map_or(Cells(0), |metrics| {
                metrics.progress_bar_width(
                    scene.settings.appearance_settings.progress_time,
                    remaining_label,
                )
            }),
            Breakpoint::Compact => compact_progress_bar_width(self.header),
            Breakpoint::Minimal => minimal_progress_bar_width(
                CardView::from_scene(scene),
                scene.settings.appearance_settings.speed_chip,
                Cells(self.screen.width),
            ),
            Breakpoint::TooSmall => Cells(0),
        }
    }
}

fn content_area(screen: Rect) -> Rect {
    let width = screen.width.saturating_sub(MARGIN * 2).min(MAX_WIDTH);
    Rect {
        y: screen.y + MARGIN,
        height: screen.height.saturating_sub(MARGIN * 2),
        ..screen
    }
    .centered_horizontally(Constraint::Length(width))
}

fn key_hint_rows(key_hints: KeyHints) -> u16 {
    match key_hints {
        KeyHints::Hidden => 0,
        KeyHints::Shown => KEY_HINTS_ROWS,
    }
}

fn header_rows(breakpoint: Breakpoint) -> u16 {
    match breakpoint {
        Breakpoint::Full => card::metrics::card_height().0,
        Breakpoint::Compact => compact_height(),
        Breakpoint::Minimal | Breakpoint::TooSmall => 0,
    }
}

fn body(scene: &Scene<'_>, screen: Rect) -> FrameLayout<'static> {
    let appearance_settings = scene.settings.appearance_settings;
    let breakpoint = Breakpoint::new(
        screen.as_size(),
        &scene.presentation.appearance.breakpoints,
        appearance_settings.layout_mode,
    );
    let content = content_area(screen);
    let header_rows = header_rows(breakpoint);
    let hint_rows = key_hint_rows(appearance_settings.key_hints);
    let [header, pane, hints] = content.layout(&Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Min(0),
        Constraint::Length(hint_rows),
    ]));
    match breakpoint {
        Breakpoint::TooSmall => FrameLayout::empty(screen, breakpoint),
        Breakpoint::Minimal => FrameLayout {
            content,
            search_bounds: pane,
            ..FrameLayout::empty(screen, breakpoint)
        },
        Breakpoint::Full | Breakpoint::Compact => {
            let card_metrics = card_metrics(scene, header, breakpoint);
            FrameLayout {
                content,
                header,
                card_metrics,
                cover_area: card_metrics
                    .map(|metrics| metrics.cover_square)
                    .filter(|cover| !cover.is_empty()),
                playlist_pane: pane,
                playlist_areas: playlist(scene, pane),
                key_hints: (hint_rows > 0).then_some(hints),
                search_bounds: pane,
                ..FrameLayout::empty(screen, breakpoint)
            }
        }
    }
}

fn card_metrics(
    scene: &Scene<'_>,
    header: Rect,
    breakpoint: Breakpoint,
) -> Option<CardMetrics> {
    match breakpoint {
        Breakpoint::Full => Some(CardMetrics::new(
            header,
            scene.presentation.cell_aspect,
            scene.cover_sizing(),
        )),
        Breakpoint::Compact | Breakpoint::Minimal | Breakpoint::TooSmall => None,
    }
}

fn playlist(scene: &Scene<'_>, pane: Rect) -> Option<PlaylistAreas> {
    if pane.is_empty() {
        return None;
    }
    if matches!(
        scene.overlay,
        Some(Overlay::Search(_) | Overlay::History(_))
    ) {
        return None;
    }
    let playlist_widget =
        PlaylistWidget::new(PlaylistView::from_scene(scene), scene.active_theme());
    Some(playlist_widget.areas(pane))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::{
        appearance::{Breakpoints, CoverMode, ProgressTime},
        cursor_over::CursorOver,
        geometry::Cells,
        index::PresetIndex,
        overlay::{Overlay, SearchQuery},
        setting_row::SettingRow,
        sleep::SleepTimer,
        time::Moment,
        toast::Toast,
    };
    use ratatui::layout::{Rect, Size};
    use rstest::rstest;

    use crate::{
        card::{
            CardView,
            compact::progress_bar_width as compact_progress_bar_width,
            metrics::CardMetrics,
        },
        overlay::modal::placement::OverlayAreas,
        primitive::bar::{hud_progress_bar_width, remaining_label},
        repaint::Presence,
        scene::PixelPath,
        screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
        test_support::{SceneSources, model_with_tracks},
    };

    fn screen() -> Rect {
        Rect::new(0, 0, 80, 24)
    }

    #[test]
    fn search_takes_the_playlist_pane() {
        let mut model = model_with_tracks(3);
        model.workspace.overlay =
            Some(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)));
        let sources = SceneSources::new(model);
        let layout = FrameLayout::from_scene(&sources.scene(), screen());
        assert_eq!(layout.playlist_areas, None);
        assert_eq!(
            layout.overlay_areas.map(OverlayAreas::outer),
            Some(layout.playlist_pane)
        );
    }

    #[test]
    fn search_bounds_stay_inside_the_content_when_the_header_overflows_it() {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.appearance_mut().breakpoints = Breakpoints {
            full_min_height: Cells(100),
            compact_min_height: Cells(1),
            min_height: Cells(1),
            ..Breakpoints::default()
        };
        let layout = FrameLayout::from_scene(&sources.scene(), Rect::new(0, 0, 60, 5));
        assert_eq!(layout.breakpoint, Breakpoint::Compact);
        assert_eq!(layout.search_bounds, layout.playlist_pane);
        assert!(layout.search_bounds.bottom() <= layout.content.bottom());
    }

    #[derive(Debug, Clone, Copy)]
    enum PlaylistPresence {
        Shown,
        Hidden,
    }

    #[rstest]
    #[case::no_overlay(None, screen(), PlaylistPresence::Shown)]
    #[case::history(
        Some(Overlay::History(CursorOver::new((), 0))),
        screen(),
        PlaylistPresence::Hidden
    )]
    #[case::settings(
        Some(Overlay::Settings(SettingRow::Theme)),
        screen(),
        PlaylistPresence::Shown
    )]
    fn layout_inputs_hide_the_playlist_under_a_taking_overlay(
        #[case] overlay: Option<Overlay>,
        #[case] area: Rect,
        #[case] playlist_presence: PlaylistPresence,
    ) {
        let mut model = model_with_tracks(3);
        model.workspace.overlay = overlay;
        let sources = SceneSources::new(model);
        let layout = FrameLayout::from_scene(&sources.scene(), area);
        match playlist_presence {
            PlaylistPresence::Shown => assert!(layout.playlist_areas.is_some()),
            PlaylistPresence::Hidden => assert!(layout.playlist_areas.is_none()),
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum CoverAvoidance {
        Avoided,
        Ignored,
    }

    #[rstest]
    #[case::vinyl(
        CoverMode::Vinyl,
        PixelPath::Protocol,
        (Presence::Shown, CoverAvoidance::Avoided)
    )]
    #[case::plain(
        CoverMode::Plain,
        PixelPath::Protocol,
        (Presence::Shown, CoverAvoidance::Avoided)
    )]
    #[case::vinyl_without_pixels(
        CoverMode::Vinyl,
        PixelPath::Halfblocks,
        (Presence::Hidden, CoverAvoidance::Avoided)
    )]
    #[case::milkdrop(
        CoverMode::Milkdrop,
        PixelPath::Halfblocks,
        (Presence::Shown, CoverAvoidance::Ignored)
    )]
    #[case::off(
        CoverMode::Off,
        PixelPath::Protocol,
        (Presence::Hidden, CoverAvoidance::Ignored)
    )]
    fn avoid_follows_the_cover_mode(
        #[case] cover_mode: CoverMode,
        #[case] pixel_path: PixelPath,
        #[case] expected: (Presence, CoverAvoidance),
    ) {
        let (cover, avoidance) = expected;
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.model.settings.appearance_settings.cover_mode = cover_mode;
        sources.pixel_path = pixel_path;
        let scene = sources.scene();
        let layout = FrameLayout::from_scene(&scene, screen());
        assert_eq!(layout.breakpoint, Breakpoint::Full);
        let cover_square = layout.card_metrics.map(|metrics| metrics.cover_square);
        let cover_area = match cover {
            Presence::Shown => cover_square,
            Presence::Hidden => None,
        };
        assert_eq!(layout.cover_area, cover_area);
        let exclusion = match avoidance {
            CoverAvoidance::Avoided => cover_area,
            CoverAvoidance::Ignored => None,
        };
        assert_eq!(layout.cover_exclusion(scene.cover_mode()), exclusion);
    }

    fn timer() -> SleepTimer {
        SleepTimer {
            preset_index: PresetIndex::new(0),
            delay: Duration::from_secs(60),
            deadline_at: Moment::new(Duration::from_secs(60)),
        }
    }

    #[rstest]
    #[case::timer_at_full(Size::new(80, 24), Some(timer()), (Breakpoint::Full, Presence::Shown))]
    #[case::timer_at_minimal(
        Size::new(20, 5),
        Some(timer()),
        (Breakpoint::Minimal, Presence::Hidden)
    )]
    #[case::no_timer(Size::new(80, 24), None, (Breakpoint::Full, Presence::Hidden))]
    fn the_sleep_label_is_on_screen_only_with_a_playlist_and_a_timer(
        #[case] size: Size,
        #[case] sleep_timer: Option<SleepTimer>,
        #[case] expected: (Breakpoint, Presence),
    ) {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.appearance_mut().breakpoints.min_width = Cells(10);
        sources.appearance_mut().breakpoints.min_height = Cells(3);
        sources.model.transport.sleep_timer = sleep_timer;
        let scene = sources.scene();
        let layout =
            FrameLayout::from_scene(&scene, Rect::new(0, 0, size.width, size.height));
        assert_eq!(
            (layout.breakpoint, layout.on_screen(&scene).sleep_label),
            expected
        );
    }

    #[rstest]
    #[case::full(CoverMode::Plain, Size::new(80, 24), (Breakpoint::Full, Presence::Shown))]
    #[case::compact(
        CoverMode::Plain,
        Size::new(80, 18),
        (Breakpoint::Compact, Presence::Hidden)
    )]
    #[case::milkdrop_at_full(
        CoverMode::Milkdrop,
        Size::new(80, 24),
        (Breakpoint::Full, Presence::Shown)
    )]
    fn the_spectrum_is_on_screen_with_a_card_spectrum_row_or_a_milkdrop_cover(
        #[case] cover_mode: CoverMode,
        #[case] size: Size,
        #[case] expected: (Breakpoint, Presence),
    ) {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.model.settings.appearance_settings.cover_mode = cover_mode;
        let scene = sources.scene();
        let layout =
            FrameLayout::from_scene(&scene, Rect::new(0, 0, size.width, size.height));
        assert_eq!(
            (layout.breakpoint, layout.on_screen(&scene).spectrum),
            expected
        );
    }

    #[rstest]
    #[case::plain_cover(CoverMode::Plain, PixelPath::Protocol, Presence::Hidden)]
    #[case::milkdrop(CoverMode::Milkdrop, PixelPath::Halfblocks, Presence::Shown)]
    fn without_a_card_spectrum_row_only_a_milkdrop_cover_shows_the_spectrum(
        #[case] cover_mode: CoverMode,
        #[case] pixel_path: PixelPath,
        #[case] expected: Presence,
    ) {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.model.settings.appearance_settings.cover_mode = cover_mode;
        sources.pixel_path = pixel_path;
        let scene = sources.scene();
        let layout = FrameLayout::from_scene(&scene, screen());
        let layout = FrameLayout {
            card_metrics: layout.card_metrics.map(|metrics| CardMetrics {
                spectrum_row: Rect::default(),
                ..metrics
            }),
            ..layout
        };
        assert_eq!(layout.breakpoint, Breakpoint::Full);
        assert_eq!(layout.on_screen(&scene).spectrum, expected);
    }

    #[rstest]
    #[case::full_with_chip(
        Size::new(80, 24),
        (48, 16),
        (Breakpoint::Full, Some(ProgressTime::Remaining))
    )]
    #[case::full_without_chip(
        Size::new(80, 24),
        (48, 16),
        (Breakpoint::Full, Some(ProgressTime::Elapsed))
    )]
    #[case::compact(Size::new(80, 18), (48, 16), (Breakpoint::Compact, None))]
    #[case::compact_with_chip(
        Size::new(80, 18),
        (48, 16),
        (Breakpoint::Compact, Some(ProgressTime::Remaining))
    )]
    #[case::minimal(Size::new(20, 5), (10, 3), (Breakpoint::Minimal, None))]
    #[case::overlay_only(Size::new(40, 10), (48, 16), (Breakpoint::TooSmall, None))]
    fn the_breakpoint_picks_the_screen_for_the_size_and_minimums(
        #[case] size: Size,
        #[case] minimums: (u16, u16),
        #[case] outcome: (Breakpoint, Option<ProgressTime>),
    ) {
        let (min_width, min_height) = minimums;
        let (breakpoint, progress_time) = outcome;
        let mut model = model_with_tracks(1);
        model.workspace.toasts = vec![Toast::info("Saved")];
        let mut sources = SceneSources::new(model);
        sources.appearance_mut().breakpoints.min_width = Cells(min_width);
        sources.appearance_mut().breakpoints.min_height = Cells(min_height);
        if let Some(progress_time) = progress_time {
            sources.model.settings.appearance_settings.progress_time = progress_time;
        }
        let scene = sources.scene();
        let layout =
            FrameLayout::from_scene(&scene, Rect::new(0, 0, size.width, size.height));
        let on_screen = layout.on_screen(&scene);

        assert_eq!(layout.breakpoint, breakpoint);
        match layout.breakpoint {
            Breakpoint::Full => {
                let metrics = layout.card_metrics.unwrap();
                let row_width = metrics.progress_row.width;
                let expected = match progress_time.unwrap() {
                    ProgressTime::Remaining => hud_progress_bar_width(
                        Cells(row_width),
                        &remaining_label(CardView::from_scene(&scene).remaining()),
                    ),
                    ProgressTime::Elapsed => Cells(row_width),
                };
                assert_eq!(on_screen.progress_bar_width, Some(expected));
                assert_eq!(on_screen.clock, Presence::Shown);
            }
            Breakpoint::Compact => {
                assert_eq!(
                    on_screen.progress_bar_width,
                    Some(compact_progress_bar_width(layout.header))
                );
                assert_eq!(on_screen.clock, Presence::Shown);
                assert_eq!(layout.remaining_label, "");
            }
            Breakpoint::Minimal => {
                assert!(on_screen.progress_bar_width.is_some());
                assert_eq!(on_screen.clock, Presence::Shown);
                assert_eq!(
                    (layout.content, layout.search_bounds),
                    (Rect::new(1, 1, 18, 3), Rect::new(1, 1, 18, 2))
                );
            }
            Breakpoint::TooSmall => {
                assert_eq!(on_screen.progress_bar_width, None);
                assert_eq!(on_screen.clock, Presence::Hidden);
                assert_eq!(layout.card_metrics, None);
                assert_eq!(layout.playlist_areas, None);
                assert_eq!(layout.toast_placement, None);
            }
        }
    }
}
