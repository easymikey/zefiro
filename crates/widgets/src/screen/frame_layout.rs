use config::{CoverStyle, KeyHints, appearance_file::WindowConfig};
use kernel::domain::Overlay;
use ratatui::layout::{Constraint, Layout, Rect};

use crate::{
    card::{self, CardMetrics, compact_height},
    overlay::{layer::OverlayLayer, modal::OverlayAreas},
    playlist::{PlaylistAreas, PlaylistPane},
    scene::Scene,
    screen::Breakpoint,
    toast::{ToastAreas, ToastCard},
};

const MAX_WIDTH: u16 = 100;
const MARGIN: u16 = 1;
const KEY_HINTS_ROWS: u16 = 1;

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
    pub fn new(scene: &Scene<'_>, screen: Rect) -> Self {
        let body = body(scene, screen);
        if body.breakpoint == Breakpoint::TooSmall {
            return body;
        }
        Self {
            overlay: overlay_layer(scene, &body).areas(screen),
            toast: toast_card(scene).and_then(|toast| toast.areas(screen)),
            ..body
        }
    }

    #[must_use]
    pub fn avoid(&self, scene: &Scene<'_>) -> Option<Rect> {
        match scene.cover_style() {
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
        Breakpoint::Full => card::height(),
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

fn body(scene: &Scene<'_>, screen: Rect) -> FrameLayout {
    let breakpoint = Breakpoint::new(screen.as_size(), &scene.appearance.layout);
    let content = content_area(screen);
    let header_rows = header_rows(breakpoint);
    let hint_rows = key_hint_rows(scene.appearance.window);
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
            playlist: playlist(scene, pane),
            key_hints: (hint_rows > 0).then_some(hints),
            search_bounds,
            ..card_areas(scene, header, empty(screen, breakpoint))
        },
    }
}

fn card_areas(scene: &Scene<'_>, header: Rect, layout: FrameLayout) -> FrameLayout {
    if layout.breakpoint != Breakpoint::Full {
        return layout;
    }
    let metrics = card::metrics(header, scene.cell_aspect, scene.cover_sizing());
    FrameLayout {
        card: Some(metrics),
        cover: Some(metrics.cover_square).filter(|cover| !cover.is_empty()),
        ..layout
    }
}

fn playlist(scene: &Scene<'_>, pane: Rect) -> Option<PlaylistAreas> {
    let taken = matches!(
        scene.model.workspace.overlay,
        Some(Overlay::Search(_) | Overlay::History(_))
    );
    if taken || pane.is_empty() {
        return None;
    }
    let playlist = PlaylistPane {
        view: scene.playlist_view(),
        theme: scene.active_theme(),
    };
    Some(playlist.areas(pane))
}

pub(crate) fn overlay_layer<'a>(
    scene: &Scene<'a>,
    layout: &FrameLayout,
) -> OverlayLayer<'a> {
    let model = scene.model;
    OverlayLayer {
        workspace: &model.workspace,
        theme: scene.active_theme(),
        tracks: &model.playlist.tracks,
        history: &model.history.view,
        settings_view: scene.settings_view(),
        bindings: scene.bindings,
        avoid: layout.avoid(scene),
        playlist_pane: layout.playlist_pane,
        search_bounds: layout.search_bounds,
        now_unix: scene.now_unix,
    }
}

pub(crate) fn toast_card<'a>(scene: &Scene<'a>) -> Option<ToastCard<'a>> {
    let toast = scene.model.workspace.toast.as_ref()?;
    Some(ToastCard {
        toast,
        theme: scene.active_theme(),
    })
}

#[cfg(test)]
mod tests {
    use config::CoverStyle;
    use kernel::domain::{CursorOver, Overlay, SearchQuery, Toast};
    use ratatui::layout::Rect;

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
        let layout = FrameLayout::new(&scene, screen());
        let card = layout.card.unwrap();
        assert_eq!(layout.breakpoint, Breakpoint::Full);
        assert_eq!(layout.cover, Some(card.cover_square));
        assert_eq!(layout.avoid(&scene), layout.cover);
        assert!(layout.playlist.is_some());
        assert!(layout.key_hints.is_some());
    }

    #[test]
    fn without_pixels_there_is_no_cover() {
        let sources = SceneSources::new(model_with_tracks(3));
        let layout = FrameLayout::new(&sources.scene(), screen());
        assert!(layout.card.is_some());
        assert_eq!(layout.cover, None);
    }

    #[test]
    fn a_text_art_cover_is_not_avoided_by_overlays() {
        let mut sources = SceneSources::new(model_with_tracks(3));
        sources.appearance.cover.style = CoverStyle::Milkdrop;
        let scene = sources.scene();
        let layout = FrameLayout::new(&scene, screen());
        assert!(layout.cover.is_some());
        assert_eq!(layout.avoid(&scene), None);
    }

    #[test]
    fn search_takes_the_playlist_pane() {
        let mut model = model_with_tracks(3);
        model.workspace.overlay =
            Some(Overlay::Search(CursorOver::new(SearchQuery::default(), 0)));
        let sources = SceneSources::new(model);
        let layout = FrameLayout::new(&sources.scene(), screen());
        assert_eq!(layout.playlist, None);
        assert_eq!(
            layout.overlay.map(OverlayAreas::painted),
            Some(layout.playlist_pane)
        );
    }

    #[test]
    fn the_toast_sits_in_the_top_right_corner_of_the_screen() {
        let mut model = model_with_tracks(3);
        model.workspace.toast = Some(Toast::info("Saved".to_string()));
        let sources = SceneSources::new(model);
        let toast = FrameLayout::new(&sources.scene(), screen()).toast.unwrap();
        assert_eq!(toast.outer.y, 0);
        assert_eq!(toast.outer.right(), screen().right());
    }

    #[test]
    fn a_terminal_below_the_minimum_has_no_rects() {
        let mut model = model_with_tracks(3);
        model.workspace.toast = Some(Toast::info("Saved".to_string()));
        let sources = SceneSources::new(model);
        let layout = FrameLayout::new(&sources.scene(), Rect::new(0, 0, 40, 10));
        assert_eq!(layout.breakpoint, Breakpoint::TooSmall);
        assert_eq!(layout.card, None);
        assert_eq!(layout.playlist, None);
        assert_eq!(layout.toast, None);
    }
}
