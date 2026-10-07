mod matches;

use std::sync::Arc;

use kernel::domain::{
    cursor_over::CursorOver,
    geometry::Cells,
    overlay::SearchQuery,
    track::Track,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Color,
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::{
        modal::{
            frame::{Modal, ModalAreas, ModalSize},
            metrics::{QUERY_ROWS, modal_title},
            place::FullWidth,
            placement::{ModalBorder, ModalContainer, OverlayAreas, leading_cells},
        },
        search::matches::{SearchMatchList, paint_match_pane, paint_match_rows},
    },
    pixels::numeric::small_count_u16,
    primitive::{
        canvas::Canvas,
        glyphs,
        list_chrome::ScrollAreas,
        span::{line, text},
    },
    theme::{active_theme::ActiveTheme, colors::Colors},
};

#[derive(Debug)]
pub(crate) struct SearchWidget<'a> {
    theme: ActiveTheme<'a>,
    tracks: &'a [Arc<Track>],
    search_query: &'a CursorOver<SearchQuery>,
    title: &'a str,
    bounds: Rect,
    container: ModalContainer<'a>,
}

impl<'a> SearchWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        query: &'a CursorOver<SearchQuery>,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            theme: active_theme,
            tracks: &[],
            search_query: query,
            title: "",
            bounds: Rect::default(),
            container: ModalContainer::Floating(&[]),
        }
    }

    #[must_use]
    pub(crate) fn tracks(mut self, tracks: &'a [Arc<Track>]) -> Self {
        self.tracks = tracks;
        self
    }

    #[must_use]
    pub(crate) fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    #[must_use]
    pub(crate) fn bounds(mut self, bounds: Rect) -> Self {
        self.bounds = bounds;
        self
    }

    #[must_use]
    pub(crate) fn container(mut self, container: ModalContainer<'a>) -> Self {
        self.container = container;
        self
    }
}

struct SearchHeader<'a> {
    query: &'a str,
    matches: usize,
    tracks_len: usize,
}

impl SearchWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        match self.container {
            ModalContainer::Floating(avoid) => {
                OverlayAreas::Dialog(self.modal().areas(screen, avoid))
            }
            ModalContainer::Playlist(pane) => {
                OverlayAreas::List(self.border(pane).areas())
            }
        }
    }

    pub(crate) fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let buffer = canvas.buffer;
        match areas {
            OverlayAreas::List(areas) => self.paint_pane(areas, buffer),
            OverlayAreas::Dialog(areas) => self.paint_modal(areas, buffer),
            OverlayAreas::Banner(_) => {}
        }
    }

    fn header(&self) -> SearchHeader<'_> {
        SearchHeader {
            query: &self.search_query.content.input,
            matches: self.search_query.content.matches.len(),
            tracks_len: self.tracks.len(),
        }
    }

    fn border(&self, area: Rect) -> ModalBorder<'_> {
        let theme = self.theme;
        ModalBorder {
            area,
            title: modal_title(glyphs::search::TITLE_WORD, self.title, theme.colors()),
            theme,
        }
    }

    fn modal(&self) -> Modal<'static> {
        let colors = self.theme.colors();
        Modal {
            title: glyphs::search::TITLE_WORD,
            size: ModalSize::FullWidth(FullWidth {
                bounds: self.bounds,
                content_rows: Cells(content_rows(self.search_query)),
            }),
            hint: None,
            border: colors.muted_foreground,
            window_background: colors.window_background,
        }
    }

    fn vertical(&self) -> Layout {
        Layout::vertical([Constraint::Length(QUERY_ROWS), Constraint::Min(0)])
    }

    fn paint_pane(&self, areas: ScrollAreas, buffer: &mut Buffer) {
        self.border(areas.outer).paint(buffer);
        let inner = areas.content;
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let vertical = self.vertical();
        let Ok([query_rect, _]) = inner.try_layout::<2>(&vertical) else {
            return;
        };
        self.paint_query(query_rect, buffer);

        let Ok([_, matches_rect]) = areas.rows.try_layout::<2>(&vertical) else {
            return;
        };
        if matches_rect.width == 0 || matches_rect.height == 0 {
            return;
        }
        paint_match_pane(
            &self.match_list(matches_rect, leading_cells(&areas).0),
            buffer,
        );
    }

    fn paint_modal(&self, areas: ModalAreas, buffer: &mut Buffer) {
        self.modal().paint(areas, buffer);
        let inner = areas.body;
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let Ok([header_area, matches_area]) =
            inner.try_layout::<2>(&Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
            ]))
        else {
            return;
        };
        Paragraph::new(header_line(&self.header(), self.theme.colors()))
            .render(header_area, buffer);
        paint_match_rows(&self.match_list(matches_area, 0), buffer);
    }

    fn paint_query(&self, area: Rect, buffer: &mut Buffer) {
        let vertical = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]);
        let Ok([query_row, rule_row]) = area.try_layout::<2>(&vertical) else {
            return;
        };
        if query_row.width > 0 && query_row.height > 0 {
            Paragraph::new(query_line(&self.header(), self.theme.colors()))
                .render(query_row, buffer);
        }
        if rule_row.width == 0 || rule_row.height == 0 {
            return;
        }
        let rule = glyphs::search::RULE.repeat(usize::from(rule_row.width));
        Paragraph::new(line([text(rule).fg(self.theme.colors().muted_foreground)]))
            .render(rule_row, buffer);
    }

    fn match_list(&self, area: Rect, lead: u16) -> SearchMatchList<'_> {
        SearchMatchList {
            area,
            tracks: self.tracks,
            search_query: self.search_query,
            colors: self.theme.colors(),
            lead,
        }
    }
}

impl Widget for &SearchWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

#[must_use]
pub(crate) fn search_title(
    search_query: &CursorOver<SearchQuery>,
    tracks_len: usize,
) -> String {
    format!(
        "{} {} {tracks_len}",
        search_query.content.matches.len(),
        glyphs::search::OF
    )
}

fn query_line<'a>(header: &SearchHeader<'a>, colors: Colors<Color>) -> Line<'a> {
    line([
        text(glyphs::search::HEADER_PREFIX).fg(colors.accent),
        text(header.query).fg(colors.foreground),
        text(glyphs::search::CURSOR).fg(colors.accent),
    ])
}

fn header_line<'a>(header: &SearchHeader<'a>, colors: Colors<Color>) -> Line<'a> {
    let summary = match_count_text(header.matches, header.tracks_len);
    let mut line = query_line(header, colors);
    line.push_span(text(glyphs::search::HEADER_GAP).fg(colors.foreground));
    line.push_span(text(summary).fg(colors.muted_foreground));
    line
}

fn match_count_text(matches: usize, tracks_len: usize) -> String {
    let noun = if matches == 1 {
        glyphs::search::MATCH_SINGULAR
    } else {
        glyphs::search::MATCH_PLURAL
    };
    format!(
        "{matches} {noun} {} {tracks_len} {}",
        glyphs::search::OF,
        glyphs::search::TOTAL
    )
}

fn content_rows(search_query: &CursorOver<SearchQuery>) -> u16 {
    let match_rows = if search_query.content.matches.is_empty() {
        1
    } else {
        small_count_u16(search_query.content.matches.len())
    };
    1u16.saturating_add(match_rows)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kernel::domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        index::ViewIndex,
        overlay::SearchQuery,
        track::{Track, TrackParts},
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{
            modal::placement::ModalContainer,
            search::{SearchWidget, search_title},
        },
        primitive::canvas::tests::find_text,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: format!("{title}.mp3").into(),
            duration: std::time::Duration::from_secs(180),
            tags: kernel::domain::track::Tags {
                title: Some(title.to_string()),
                ..kernel::domain::track::Tags::default()
            },
            audio_format: kernel::domain::track::AudioFormat::default(),
        }))
    }

    fn query(
        input: &str,
        matches: Vec<usize>,
        selected_index: usize,
    ) -> CursorOver<SearchQuery> {
        let length = matches.len();
        CursorOver {
            cursor: Cursor::at(length, selected_index),
            content: SearchQuery {
                input: input.to_string(),
                matches: matches.into_iter().map(ViewIndex::new).collect(),
            },
        }
    }

    fn pane_container(pane: Rect) -> ModalContainer<'static> {
        ModalContainer::Playlist(pane)
    }

    #[test]
    fn search_overlay_shows_the_query_and_narrows_the_matches_in_pane_mode() {
        let theme = noir();
        let tracks = [
            titled_track("Moon River"),
            titled_track("Sun Song"),
            titled_track("Moonlight Sonata"),
        ];
        let search = query("moon", vec![0, 2], 0);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 80, 28))
                .container(pane_container(Rect::new(0, 0, 80, 28)));
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn search_overlay_shows_a_placeholder_when_there_are_no_matches() {
        let theme = noir();
        let tracks = [titled_track("Alpha")];
        let search = query("zz", vec![], 0);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 80, 28))
                .container(pane_container(Rect::new(0, 0, 80, 28)));
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[test]
    fn search_overlay_header_counts_matches_of_the_whole_playlist() {
        let theme = noir();
        let tracks: Vec<Arc<Track>> = (0..1961).map(|_| titled_track("Song")).collect();
        let search = query("moon", (0..7).collect(), 0);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 80, 28))
                .container(pane_container(Rect::new(0, 0, 80, 28)));
        insta::assert_snapshot!(
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .to_string()
        );
    }

    #[rstest]
    #[case::singular("singular", vec![0], 0)]
    #[case::plural_with_a_selection("plural", vec![0, 1], 1)]
    fn search_overlay_modal_mode_reports_the_match_count(
        #[case] label: &str,
        #[case] matches: Vec<usize>,
        #[case] selected_index: usize,
    ) {
        let theme = noir();
        let tracks = [titled_track("Alpha"), titled_track("Beta")];
        let search = query("a", matches, selected_index);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 80, 28))
                .container(ModalContainer::Floating(&[]));
        insta::with_settings!({ snapshot_suffix => label }, {
            insta::assert_snapshot!(rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area())).to_string());
        });
    }

    #[test]
    fn search_overlay_highlights_the_selected_match() {
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let tracks = [titled_track("Alpha"), titled_track("Beta")];
        let search = query("a", vec![0, 1], 1);
        let title = search_title(&search, tracks.len());
        let overlay = SearchWidget::new(&search, active_theme)
            .title(&title)
            .tracks(&tracks)
            .bounds(Rect::new(0, 0, 80, 28))
            .container(pane_container(Rect::new(0, 0, 80, 28)));
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let selection_background = active_theme.colors().selection_background;
        let (alpha_x, alpha_y) = find_text(&buffer, "Alpha").unwrap();
        let (beta_x, beta_y) = find_text(&buffer, "Beta").unwrap();
        assert_eq!(
            buffer[(beta_x, beta_y)].style().bg,
            Some(selection_background)
        );
        assert_ne!(
            buffer[(alpha_x, alpha_y)].style().bg,
            Some(selection_background)
        );
    }

    #[test]
    fn search_overlay_pane_mode_frames_the_given_pane_rect() {
        let theme = noir();
        let tracks = [titled_track("Track")];
        let search = query("t", vec![0], 0);
        let pane = Rect::new(0, 0, 80, 28);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(pane)
                .container(pane_container(pane));
        assert_eq!(overlay.areas(pane).outer(), pane);
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let (_, title_row) = find_text(&buffer, "SEARCH").unwrap();
        assert_eq!(title_row, pane.y);
    }

    #[rstest]
    #[case::just_below_the_row_limit(65534)]
    #[case::at_the_row_limit(65535)]
    #[case::above_the_row_limit(70000)]
    fn search_overlay_modal_mode_does_not_panic_on_a_huge_match_count(
        #[case] count: usize,
    ) {
        let theme = noir();
        let tracks = [titled_track("Alpha")];
        let search = query("", vec![0; count], count - 1);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 80, 28))
                .container(ModalContainer::Floating(&[]));
        let backend =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()));
        assert_eq!(backend.to_string().lines().count(), 28);
    }

    #[rstest]
    #[case::pane(true)]
    #[case::modal(false)]
    fn search_overlay_scrolls_the_selected_match_into_view(#[case] pane: bool) {
        let theme = noir();
        let tracks: Vec<Arc<Track>> = (0..100)
            .map(|index| titled_track(&format!("Song {index:03}")))
            .collect();
        let search = query("song", (0..100).collect(), 99);
        let container = if pane {
            pane_container(Rect::new(0, 0, 80, 28))
        } else {
            ModalContainer::Floating(&[])
        };
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 80, 28))
                .container(container);
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        assert!(find_text(&buffer, "Song 099").is_some());
        assert!(find_text(&buffer, "Song 000").is_none());
    }

    #[rstest]
    #[case::modal(None)]
    #[case::pane(Some(Rect::new(0, 0, 4, 3)))]
    fn search_overlay_does_not_panic_on_a_tiny_terminal(#[case] pane: Option<Rect>) {
        let theme = noir();
        let tracks: [Arc<Track>; 0] = [];
        let search = CursorOver::default();
        let container = pane.map_or(ModalContainer::Floating(&[]), pane_container);
        let title = search_title(&search, tracks.len());
        let overlay =
            SearchWidget::new(&search, ActiveTheme::new(&theme, ColorDepth::TrueColor))
                .title(&title)
                .tracks(&tracks)
                .bounds(Rect::new(0, 0, 4, 3))
                .container(container);
        let backend =
            rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()));
        assert_eq!(backend.to_string().lines().count(), 3);
    }
}
