mod matches;

use std::sync::Arc;

use kernel::domain::{CursorOver, SearchQuery, Track};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{
    overlay::{
        modal::{
            Modal,
            ModalBorder,
            ModalBounds,
            ModalChrome,
            ModalMetrics,
            ModalRowColors,
            ModalScrollAreas,
            ModalSize,
            OverlayAreas,
            OverlayContainer,
            PlacedModal,
            lead_cells,
            modal_title,
        },
        search::matches::{SearchMatchList, render_match_list, render_matches},
    },
    primitive::{
        canvas::Canvas,
        glyphs::SearchGlyphs,
        inset::Inset,
        span::{row, text},
    },
    theme::ActiveTheme,
};

#[derive(Debug)]
pub(crate) struct SearchOverlay<'a> {
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

impl SearchOverlay<'_> {
    #[must_use]
    pub(crate) fn areas(&self, screen: Rect) -> OverlayAreas {
        match self.container {
            OverlayContainer::Modal { avoid } => {
                OverlayAreas::Dialog(self.modal().frame(screen, avoid))
            }
            OverlayContainer::Pane(pane) => {
                OverlayAreas::List(self.border(pane).areas())
            }
        }
    }

    pub(crate) fn render_in(&self, areas: OverlayAreas, canvas: Canvas<'_>) {
        let Canvas { area, buffer } = canvas;
        match areas {
            OverlayAreas::List(areas) => self.render_pane(areas, buffer),
            OverlayAreas::Dialog(areas) => self.render_modal(
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
            OverlayContainer::Modal { avoid } => avoid,
            OverlayContainer::Pane(_) => &[],
        }
    }

    fn colors(&self) -> ModalRowColors {
        ModalRowColors::from_theme(&self.theme)
    }

    fn header(&self) -> SearchHeader<'_> {
        SearchHeader {
            query: &self.search.rows.input,
            matches: self.search.rows.matches.len(),
            total: self.tracks.len(),
        }
    }

    fn border(&self, area: Rect) -> ModalBorder<'_> {
        let theme = self.theme;
        ModalBorder {
            area,
            title: search_title(&self.header(), theme),
            inset: Inset::default(),
            theme,
            chrome: ModalChrome::default(),
        }
    }

    fn modal(&self) -> Modal<'static> {
        let theme = self.theme;
        Modal {
            title: SearchGlyphs::default().title_word,
            size: ModalSize::FrameWidth {
                bounds: self.bounds,
                content_rows: content_rows(self.search),
            },
            hint: None,
            border: theme.frame(),
            window_background: theme.window_bg(),
        }
    }

    fn vertical(&self) -> Layout {
        Layout::vertical([
            Constraint::Length(ModalMetrics::default().query_rows),
            Constraint::Min(0),
        ])
    }

    fn render_pane(&self, areas: ModalScrollAreas, buffer: &mut Buffer) {
        self.border(areas.outer).paint(buffer);
        let inner = areas.content;
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let vertical = self.vertical();
        let Ok([query_rect, _]) = inner.try_layout::<2>(&vertical) else {
            return;
        };
        self.render_query(query_rect, buffer);

        let Ok([_, matches_rect]) = areas.rows.try_layout::<2>(&vertical) else {
            return;
        };
        if matches_rect.width == 0 || matches_rect.height == 0 {
            return;
        }
        render_match_list(&self.match_list(matches_rect, lead_cells(&areas)), buffer);
    }

    fn render_modal(&self, placed: PlacedModal<'_>, buffer: &mut Buffer) {
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
        Paragraph::new(header_line(&self.header(), self.colors()))
            .render(header_area, buffer);
        render_matches(&self.match_list(matches_area, 0), buffer);
    }

    fn render_query(&self, area: Rect, buffer: &mut Buffer) {
        let vertical = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]);
        let Ok([query_row, rule_row]) = area.try_layout::<2>(&vertical) else {
            return;
        };
        if query_row.width > 0 && query_row.height > 0 {
            Paragraph::new(query_line(&self.header(), self.colors()))
                .render(query_row, buffer);
        }
        if rule_row.width == 0 || rule_row.height == 0 {
            return;
        }
        let rule = SearchGlyphs::default()
            .rule
            .repeat(usize::from(rule_row.width));
        let border = self.theme.frame();
        Paragraph::new(row([text(rule).fg(border)])).render(rule_row, buffer);
    }

    fn match_list(&self, area: Rect, lead: u16) -> SearchMatchList<'_> {
        SearchMatchList {
            area,
            tracks: self.tracks,
            search: self.search,
            colors: self.colors(),
            lead,
            scroll_padding: ModalMetrics::default().scroll_padding,
        }
    }
}

impl Widget for &SearchOverlay<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.render_in(self.areas(area), Canvas { area, buffer });
    }
}

fn search_title(header: &SearchHeader<'_>, theme: ActiveTheme<'_>) -> Line<'static> {
    let glyphs = SearchGlyphs::default();
    modal_title(
        glyphs.title_word,
        format!("{} {} {}", header.matches, glyphs.of, header.total),
        theme,
    )
}

fn query_line(header: &SearchHeader<'_>, colors: ModalRowColors) -> Line<'static> {
    let glyphs = SearchGlyphs::default();
    row([
        text(glyphs.header_prefix).fg(colors.accent),
        text(header.query.to_string()).fg(colors.text),
        text(glyphs.cursor).fg(colors.accent),
    ])
}

fn header_line(header: &SearchHeader<'_>, colors: ModalRowColors) -> Line<'static> {
    let glyphs = SearchGlyphs::default();
    let summary = match_count_text(header.matches, header.total, glyphs);
    row([
        text(glyphs.header_prefix).fg(colors.accent),
        text(header.query.to_string()).fg(colors.text),
        text(glyphs.cursor).fg(colors.accent),
        text(glyphs.header_gap).fg(colors.text),
        text(summary).fg(colors.dim),
    ])
}

fn match_count_text(matches: usize, total: usize, glyphs: SearchGlyphs) -> String {
    let noun = if matches == 1 {
        glyphs.match_singular
    } else {
        glyphs.match_plural
    };
    format!("{matches} {noun} {} {total} {}", glyphs.of, glyphs.total)
}

fn content_rows(search: &CursorOver<SearchQuery>) -> u16 {
    let match_rows = if search.rows.matches.is_empty() {
        1
    } else {
        u16::try_from(search.rows.matches.len()).unwrap_or(u16::MAX)
    };
    1 + match_rows
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{AudioFormat, Cursor, CursorOver, SearchQuery, Tags, Track};
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::{
        overlay::{modal::OverlayContainer, search::SearchOverlay},
        scene::fixtures::{find_text, noir, painted, painted_buffer},
        theme::{ActiveTheme, ColorDepth},
    };

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("{title}.mp3"))
                .duration(Duration::from_secs(180))
                .tags(Tags {
                    title: Some(title.to_string()),
                    ..Tags::default()
                })
                .audio_format(AudioFormat::default())
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
            rows: SearchQuery {
                input: input.to_string(),
                matches,
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
        let overlay = SearchOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn search_overlay_shows_a_placeholder_when_there_are_no_matches() {
        let theme = noir();
        let tracks = [titled_track("Alpha")];
        let search = query("zz", vec![], 0);
        let overlay = SearchOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
        insta::assert_snapshot!(painted(&overlay, 80, 28));
    }

    #[test]
    fn search_overlay_header_counts_matches_of_the_whole_playlist() {
        let theme = noir();
        let tracks: Vec<Arc<Track>> = (0..1961).map(|_| titled_track("Song")).collect();
        let search = query("moon", (0..7).collect(), 0);
        let overlay = SearchOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
        insta::assert_snapshot!(painted(&overlay, 80, 28));
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
        let overlay = SearchOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: OverlayContainer::Modal { avoid: &[] },
        };
        insta::with_settings!({ snapshot_suffix => label }, {
            insta::assert_snapshot!(painted(&overlay, 80, 28));
        });
    }

    #[test]
    fn search_overlay_highlights_the_selected_match() {
        let theme = noir();
        let active = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let tracks = [titled_track("Alpha"), titled_track("Beta")];
        let search = query("a", vec![0, 1], 1);
        let overlay = SearchOverlay {
            theme: active,
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 80, 28),
            container: pane_container(Rect::new(0, 0, 80, 28)),
        };
        let buffer = painted_buffer(&overlay, 80, 28);
        let selection_bg = active.selection_bg();
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
        let overlay = SearchOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: pane,
            container: pane_container(pane),
        };
        assert_eq!(overlay.areas(pane).painted(), pane);
        let buffer = painted_buffer(&overlay, 80, 28);
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
        let container =
            pane.map_or(OverlayContainer::Modal { avoid: &[] }, pane_container);
        let overlay = SearchOverlay {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            tracks: &tracks,
            search: &search,
            bounds: Rect::new(0, 0, 4, 3),
            container,
        };
        let _ = painted(&overlay, 4, 3);
    }
}
