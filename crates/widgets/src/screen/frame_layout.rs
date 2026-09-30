use config::{CoverStyle, KeyHints, LayoutConfig, WindowConfig};
use ratatui::layout::{Constraint, Layout, Rect};

use crate::{
    card::{self, CardMetrics, compact_height},
    geometry::{CellAspect, CoverSizing},
    overlay::{
        layer::{OverlayContent, OverlayLayer},
        modal::OverlayAreas,
    },
    playlist::{PlaylistAreas, PlaylistPane},
    screen::Breakpoint,
    toast::{ToastAreas, ToastCard},
};

const MAX_WIDTH: u16 = 100;
const MARGIN: u16 = 1;
const KEY_HINTS_ROWS: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub struct LayoutInputs<'a> {
    pub layout: &'a LayoutConfig,
    pub window: WindowConfig,
    pub cell_aspect: CellAspect,
    pub cover_sizing: CoverSizing,
    pub cover_style: CoverStyle,
    pub(crate) playlist: Option<PlaylistPane<'a>>,
    pub(crate) overlay: OverlayContent<'a>,
    pub(crate) toast: Option<ToastCard<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLayout {
    pub screen: Rect,
    pub breakpoint: Breakpoint,
    pub content: Rect,
    pub header: Rect,
    pub card: Option<CardMetrics>,
    pub cover: Option<Rect>,
    pub playlist_pane: Rect,
    pub playlist: Option<PlaylistAreas>,
    pub key_hints: Option<Rect>,
    pub search_bounds: Rect,
    pub overlay: Option<OverlayAreas>,
    pub toast: Option<ToastAreas>,
}

impl FrameLayout {
    #[must_use]
    pub fn new(inputs: &LayoutInputs<'_>, screen: Rect) -> Self {
        let body = body(inputs, screen);
        if body.breakpoint == Breakpoint::TooSmall {
            return body;
        }
        Self {
            overlay: OverlayLayer::placed(inputs.overlay, &body, inputs.cover_style)
                .areas(screen),
            toast: inputs.toast.and_then(|toast| toast.areas(screen)),
            ..body
        }
    }

    #[must_use]
    pub fn cover_exclusion(&self, cover_style: CoverStyle) -> Option<Rect> {
        match cover_style {
            CoverStyle::Vinyl | CoverStyle::Plain => self.cover,
            CoverStyle::Milkdrop | CoverStyle::Off => None,
        }
    }
}

fn empty(screen: Rect, breakpoint: Breakpoint) -> FrameLayout {
    FrameLayout {
        screen,
        breakpoint,
        content: Rect::default(),
        header: Rect::default(),
        card: None,
        cover: None,
        playlist_pane: Rect::default(),
        playlist: None,
        key_hints: None,
        search_bounds: Rect::default(),
        overlay: None,
        toast: None,
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

fn key_hint_rows(window: WindowConfig) -> u16 {
    match window.key_hints {
        KeyHints::Hidden => 0,
        KeyHints::Shown => KEY_HINTS_ROWS,
    }
}

fn header_rows(breakpoint: Breakpoint) -> u16 {
    match breakpoint {
        Breakpoint::Full => card::card_height(),
        Breakpoint::Compact => compact_height(),
        Breakpoint::Minimal | Breakpoint::TooSmall => 0,
    }
}

fn search_bounds(content: Rect, header_rows: u16, hint_rows: u16) -> Rect {
    let top = content.y.saturating_add(header_rows);
    let bottom = content.bottom().saturating_sub(hint_rows);
    Rect {
        y: top,
        height: bottom.saturating_sub(top),
        ..content
    }
}

fn body(inputs: &LayoutInputs<'_>, screen: Rect) -> FrameLayout {
    let breakpoint = Breakpoint::new(screen.as_size(), inputs.layout);
    let content = content_area(screen);
    let header_rows = header_rows(breakpoint);
    let hint_rows = key_hint_rows(inputs.window);
    let [header, pane, hints] = content.layout(&Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Min(0),
        Constraint::Length(hint_rows),
    ]));
    let search_bounds = search_bounds(content, header_rows, hint_rows);
    match breakpoint {
        Breakpoint::TooSmall => empty(screen, breakpoint),
        Breakpoint::Minimal => FrameLayout {
            content,
            search_bounds,
            ..empty(screen, breakpoint)
        },
        Breakpoint::Full | Breakpoint::Compact => FrameLayout {
            content,
            header,
            playlist_pane: pane,
            playlist: playlist(inputs, pane),
            key_hints: (hint_rows > 0).then_some(hints),
            search_bounds,
            ..card_areas(inputs, header, empty(screen, breakpoint))
        },
    }
}

fn card_areas(
    inputs: &LayoutInputs<'_>,
    header: Rect,
    layout: FrameLayout,
) -> FrameLayout {
    if layout.breakpoint != Breakpoint::Full {
        return layout;
    }
    let metrics = card::card_metrics(header, inputs.cell_aspect, inputs.cover_sizing);
    FrameLayout {
        card: Some(metrics),
        cover: Some(metrics.cover_square).filter(|cover| !cover.is_empty()),
        ..layout
    }
}

fn playlist(inputs: &LayoutInputs<'_>, pane: Rect) -> Option<PlaylistAreas> {
    if pane.is_empty() {
        return None;
    }
    let playlist = inputs.playlist?;
    Some(playlist.areas(pane))
}

#[cfg(test)]
mod tests {
    use config::CoverStyle;
    use kernel::domain::{CursorOver, Overlay, SearchQuery, SettingRow, Toast};
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::modal::OverlayAreas,
        scene::{
            PixelPath,
            Scene,
            fixtures::{SceneSources, model_with_tracks},
        },
        screen::{Breakpoint, FrameLayout},
    };

    fn screen() -> Rect {
        Rect::new(0, 0, 80, 24)
    }

    fn with_pixels(scene: Scene<'_>) -> Scene<'_> {
        Scene {
            pixel_path: PixelPath::Protocol,
            ..scene
        }
    }

    #[test]
    fn a_full_frame_with_pixels_holds_the_card_and_the_cover() {
        let sources = SceneSources::new(model_with_tracks(3));
        let scene = with_pixels(sources.scene());
        let layout = FrameLayout::new(&scene.layout_inputs(), screen());
        let card = layout.card.unwrap();
        assert_eq!(layout.breakpoint, Breakpoint::Full);
        assert_eq!(layout.cover, Some(card.cover_square));
        assert_eq!(layout.cover_exclusion(scene.cover_style()), layout.cover);
        assert!(layout.playlist.is_some());
        assert!(layout.key_hints.is_some());
    }

    #[test]
    fn without_pixels_there_is_no_cover() {
        let sources = SceneSources::new(model_with_tracks(3));
        let layout = FrameLayout::new(&sources.scene().layout_inputs(), screen());
        assert!(layout.card.is_some());
        assert_eq!(layout.cover, None);
    }

    #[test]
    fn a_text_art_cover_is_not_avoided_by_overlays() {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.appearance.cover.style = CoverStyle::Milkdrop;
        let scene = sources.scene();
        let layout = FrameLayout::new(&scene.layout_inputs(), screen());
        assert!(layout.cover.is_some());
        assert_eq!(layout.cover_exclusion(scene.cover_style()), None);
    }

    #[test]
    fn search_takes_the_playlist_pane() {
        let mut model = model_with_tracks(3);
        model.workspace.overlay =
            Some(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)));
        let sources = SceneSources::new(model);
        let layout = FrameLayout::new(&sources.scene().layout_inputs(), screen());
        assert_eq!(layout.playlist, None);
        assert_eq!(
            layout.overlay.map(OverlayAreas::outer),
            Some(layout.playlist_pane)
        );
    }

    #[test]
    fn the_toast_sits_in_the_top_right_corner_of_the_screen() {
        let mut model = model_with_tracks(3);
        model.workspace.toast = Some(Toast::info("Saved".to_string()));
        let sources = SceneSources::new(model);
        let toast = FrameLayout::new(&sources.scene().layout_inputs(), screen())
            .toast
            .unwrap();
        assert_eq!(toast.outer.y, 0);
        assert_eq!(toast.outer.right(), screen().right());
    }

    #[test]
    fn a_terminal_below_the_minimum_has_no_rects() {
        let mut model = model_with_tracks(3);
        model.workspace.toast = Some(Toast::info("Saved".to_string()));
        let sources = SceneSources::new(model);
        let layout =
            FrameLayout::new(&sources.scene().layout_inputs(), Rect::new(0, 0, 40, 10));
        assert_eq!(layout.breakpoint, Breakpoint::TooSmall);
        assert_eq!(layout.card, None);
        assert_eq!(layout.playlist, None);
        assert_eq!(layout.toast, None);
    }

    #[derive(Debug, Clone, Copy)]
    enum PlaylistPresence {
        Shown,
        Hidden,
    }

    #[rstest]
    #[case::no_overlay(None, screen(), PlaylistPresence::Shown)]
    #[case::search(
        Some(Overlay::Search(CursorOver::new(SearchQuery::default(), 0))),
        screen(),
        PlaylistPresence::Hidden
    )]
    #[case::history(
        Some(Overlay::History(CursorOver::new((), 0))),
        screen(),
        PlaylistPresence::Hidden
    )]
    #[case::settings(
        Some(Overlay::Settings {
            selected: SettingRow::Theme
        }),
        screen(),
        PlaylistPresence::Shown
    )]
    #[case::empty_pane(None, Rect::new(0, 0, 40, 10), PlaylistPresence::Hidden)]
    fn layout_inputs_hide_the_playlist_under_a_taking_overlay(
        #[case] overlay: Option<Overlay>,
        #[case] area: Rect,
        #[case] presence: PlaylistPresence,
    ) {
        let mut model = model_with_tracks(3);
        model.workspace.overlay = overlay;
        let sources = SceneSources::new(model);
        let layout = FrameLayout::new(&sources.scene().layout_inputs(), area);
        match presence {
            PlaylistPresence::Shown => assert!(layout.playlist.is_some()),
            PlaylistPresence::Hidden => assert!(layout.playlist.is_none()),
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum CoverAvoidance {
        Avoided,
        Ignored,
    }

    #[rstest]
    #[case::vinyl(CoverStyle::Vinyl, CoverAvoidance::Avoided)]
    #[case::plain(CoverStyle::Plain, CoverAvoidance::Avoided)]
    #[case::milkdrop(CoverStyle::Milkdrop, CoverAvoidance::Ignored)]
    #[case::off(CoverStyle::Off, CoverAvoidance::Ignored)]
    fn avoid_follows_the_cover_style(
        #[case] style: CoverStyle,
        #[case] avoidance: CoverAvoidance,
    ) {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.appearance.cover.style = style;
        let scene = with_pixels(sources.scene());
        let layout = FrameLayout::new(&scene.layout_inputs(), screen());
        let expected = match avoidance {
            CoverAvoidance::Avoided => layout.cover,
            CoverAvoidance::Ignored => None,
        };
        assert_eq!(layout.cover_exclusion(style), expected);
    }
}
