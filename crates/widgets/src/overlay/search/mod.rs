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
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::{
        modal::{
            frame::{Modal, ModalBounds, ModalSize, PlacedModal},
            metrics::{ModalRowStyle, QUERY_ROWS, SCROLL_PADDING, modal_title},
            placement::{
                ModalBorder,
                ModalScrollAreas,
                OverlayAreas,
                OverlayContainer,
                leading_cells,
            },
        },
        search::matches::{SearchMatchList, paint_match_pane, paint_match_rows},
    },
    primitive::{
        canvas::Canvas,
        glyphs,
        inset::Inset,
        span::{line, text},
    },
    theme::active_theme::ActiveTheme,
};

#[derive(Debug)]
pub(crate) struct SearchWidget<'a> {
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) tracks: &'a [Arc<Track>],
    pub(crate) search: &'a CursorOver<SearchQuery>,
    pub(crate) bounds: Rect,
    pub(crate) container: OverlayContainer<'a>,
}

struct SearchHeader<'a> {
    query: &'a str,
    matches: usize,
    total: usize,
}

impl SearchWidget<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        match self.container {
            OverlayContainer::Modal(avoid) => {
                OverlayAreas::Dialog(self.modal().areas(screen, avoid))
            }
            OverlayContainer::Pane(pane) => {
                OverlayAreas::List(self.border(pane).areas())
            }
        }
    }

    fn paint(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        match areas {
            OverlayAreas::List(areas) => self.paint_pane(areas, buffer),
            OverlayAreas::Dialog(areas) => self.paint_modal(
                PlacedModal {
                    areas,
                    bounds: ModalBounds {
                        area,
                        avoid: self.avoid(),
                    },
                },
                buffer,
            ),
            OverlayAreas::Banner(_) => {}
        }
    }

    fn avoid(&self) -> &[Rect] {
        match self.container {
            OverlayContainer::Modal(avoid) => avoid,
            OverlayContainer::Pane(_) => &[],
        }
    }

    fn style(&self) -> ModalRowStyle {
        ModalRowStyle::from_theme(&self.theme)
    }

    fn header(&self) -> SearchHeader<'_> {
        SearchHeader {
            query: &self.search.content.input,
            matches: self.search.content.matches.len(),
            total: self.tracks.len(),
        }
    }

    fn border(&self, area: Rect) -> ModalBorder<'_> {
        let theme = self.theme;
        ModalBorder {
            area,
            title: search_title(&self.header(), theme),
            inset: Inset::overlay(),
            theme,
        }
    }

    fn modal(&self) -> Modal<'static> {
        let style = self.style();
        Modal {
            title: glyphs::search::TITLE_WORD,
            size: ModalSize::FrameWidth {
                bounds: self.bounds,
                content_rows: Cells(content_rows(self.search)),
            },
            hint: None,
            border: style.border,
            window_background: style.background,
        }
    }

    fn vertical(&self) -> Layout {
        Layout::vertical([Constraint::Length(QUERY_ROWS), Constraint::Min(0)])
    }

    fn paint_pane(&self, areas: ModalScrollAreas, buffer: &mut Buffer) {
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

    fn paint_modal(&self, placed: PlacedModal<'_>, buffer: &mut Buffer) {
        self.modal().paint(placed, buffer);
        let inner = placed.areas.body;
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
        Paragraph::new(header_line(&self.header(), self.style()))
            .render(header_area, buffer);
        paint_match_rows(&self.match_list(matches_area, 0), buffer);
    }

    fn paint_query(&self, area: Rect, buffer: &mut Buffer) {
        let vertical = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]);
        let Ok([query_row, rule_row]) = area.try_layout::<2>(&vertical) else {
            return;
        };
        if query_row.width > 0 && query_row.height > 0 {
            Paragraph::new(query_line(&self.header(), self.style()))
                .render(query_row, buffer);
        }
        if rule_row.width == 0 || rule_row.height == 0 {
            return;
        }
        let rule = glyphs::search::RULE.repeat(usize::from(rule_row.width));
        Paragraph::new(line([text(rule).fg(self.style().border)]))
            .render(rule_row, buffer);
    }

    fn match_list(&self, area: Rect, lead: u16) -> SearchMatchList<'_> {
        SearchMatchList {
            area,
            tracks: self.tracks,
            search: self.search,
            style: self.style(),
            lead,
            scroll_padding: SCROLL_PADDING,
        }
    }
}

impl Widget for &SearchWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.paint(self.areas(area), Canvas { area, buffer });
    }
}

fn search_title(header: &SearchHeader<'_>, theme: ActiveTheme<'_>) -> Line<'static> {
    modal_title(
        glyphs::search::TITLE_WORD,
        format!("{} {} {}", header.matches, glyphs::search::OF, header.total),
        ModalRowStyle::from_theme(&theme),
    )
}

fn query_line(header: &SearchHeader<'_>, style: ModalRowStyle) -> Line<'static> {
    line([
        text(glyphs::search::HEADER_PREFIX).fg(style.accent),
        text(header.query.to_string()).fg(style.foreground),
        text(glyphs::search::CURSOR).fg(style.accent),
    ])
}

fn header_line(header: &SearchHeader<'_>, style: ModalRowStyle) -> Line<'static> {
    let summary = match_count_text(header.matches, header.total);
    line([
        text(glyphs::search::HEADER_PREFIX).fg(style.accent),
        text(header.query.to_string()).fg(style.foreground),
        text(glyphs::search::CURSOR).fg(style.accent),
        text(glyphs::search::HEADER_GAP).fg(style.foreground),
        text(summary).fg(style.muted_foreground),
    ])
}

fn match_count_text(matches: usize, total: usize) -> String {
    let noun = if matches == 1 {
        glyphs::search::MATCH_SINGULAR
    } else {
        glyphs::search::MATCH_PLURAL
    };
    format!(
        "{matches} {noun} {} {total} {}",
        glyphs::search::OF,
        glyphs::search::TOTAL
    )
}

fn content_rows(search: &CursorOver<SearchQuery>) -> u16 {
    let match_rows = if search.content.matches.is_empty() {
        1
    } else {
        u16::try_from(search.content.matches.len()).unwrap_or(u16::MAX)
    };
    1 + match_rows
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kernel::domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        index::ViewIndex,
        overlay::SearchQuery,
        track::Track,
    };
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{
            modal::{metrics::ModalRowStyle, placement::OverlayContainer},
            search::SearchWidget,
        },
        primitive::canvas::find_text,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("{title}.mp3"))
                .duration(std::time::Duration::from_secs(180))
                .tags(kernel::domain::track::Tags {
                    title: Some(title.to_string()),
                    ..kernel::domain::track::Tags::default()
                })
                .audio_format(kernel::domain::track::AudioFormat::default())
                .build(),
        )
    }

    fn query(
        input: &str,
        matches: Vec<usize>,
        selected: usize,
    ) -> CursorOver<SearchQuery> {
        let length = matches.len();
        CursorOver {
            cursor: Cursor::with_len(length).at(selected),
            content: SearchQuery {
                input: input.to_string(),
                matches: matches.into_iter().map(ViewIndex::new).collect(),
            },
        }
    }

    fn pane_container(pane: Rect) -> OverlayContainer<'static> {
        OverlayContainer::Pane(pane)
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
        let overlay = SearchWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
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
        let overlay = SearchWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
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
        let overlay = SearchWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
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
        #[case] selected: usize,
    ) {
        let theme = noir();
        let tracks = [titled_track("Alpha"), titled_track("Beta")];
        let search = query("a", matches, selected);
        let overlay = SearchWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: OverlayContainer::Modal(&[]),
        };
        insta::with_settings!({ snapshot_suffix => label }, {
            insta::assert_snapshot!(rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area())).to_string());
        });
    }

    #[test]
    fn search_overlay_highlights_the_selected_match() {
        let theme = noir();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let tracks = [titled_track("Alpha"), titled_track("Beta")];
        let search = query("a", vec![0, 1], 1);
        let overlay = SearchWidget {
            theme: active,
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let selection_bg = ModalRowStyle::from_theme(&active).selected_background;
        let (alpha_x, alpha_y) = find_text(&buffer, "Alpha").unwrap();
        let (beta_x, beta_y) = find_text(&buffer, "Beta").unwrap();
        assert_eq!(buffer[(beta_x, beta_y)].style().bg, Some(selection_bg));
        assert_ne!(buffer[(alpha_x, alpha_y)].style().bg, Some(selection_bg));
    }

    #[test]
    fn search_overlay_pane_mode_frames_the_given_pane_rect() {
        let theme = noir();
        let tracks = [titled_track("Track")];
        let search = query("t", vec![0], 0);
        let pane = Rect::new(0, 0, 80, 28);
        let overlay = SearchWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: pane,
            container: pane_container(pane),
        };
        assert_eq!(overlay.areas(pane).outer(), pane);
        let buffer =
            rendered(80, 28, |frame| frame.render_widget(&overlay, frame.area()))
                .buffer()
                .clone();
        let (_, title_row) = find_text(&buffer, "SEARCH").unwrap();
        assert_eq!(title_row, pane.y);
    }

    #[rstest]
    #[case::modal(None)]
    #[case::pane(Some(Rect::new(0, 0, 4, 3)))]
    fn search_overlay_does_not_panic_on_a_tiny_terminal(#[case] pane: Option<Rect>) {
        let theme = noir();
        let tracks: [Arc<Track>; 0] = [];
        let search = CursorOver::default();
        let container = pane.map_or(OverlayContainer::Modal(&[]), pane_container);
        let overlay = SearchWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 4, 3),
            container,
        };
        let backend =
            rendered(4, 3, |frame| frame.render_widget(&overlay, frame.area()));
        assert_eq!(backend.to_string().lines().count(), 3);
    }
}
