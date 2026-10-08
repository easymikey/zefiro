use std::iter::once;

use kernel::domain::{
    catalog::Paging,
    geometry::Cells,
    index::ViewIndex,
    server::{AlbumOrder, Listing, Server, ServerAlbum, ServerStatus},
    track::CatalogRow,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{HighlightSpacing, List, ListState, Paragraph, StatefulWidget, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    playlist::{
        chrome::{pane_block, title_budget},
        row::{PlaylistAreas, WindowFit, cursor_band, row_window},
        view::CatalogView,
    },
    primitive::{
        glyphs::{DOT_SEPARATOR, OFFLINE_GLYPH},
        list_chrome::{Scrollbar, paint_scrollbar, scroll_areas},
        marker::MARKERS_WIDTH,
        span::{StyledText, line, text},
        time_text::duration_text,
        track_row::{Playing, Selected, TrackRow, row_style, track_row_line},
        truncate::{blanks, truncate_line},
    },
    theme::{active_theme::ActiveTheme, colors::Colors},
};

const GAP: usize = 1;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CatalogWidget<'a> {
    catalog_view: CatalogView<'a>,
    active_theme: ActiveTheme<'a>,
}

impl<'a> CatalogWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        catalog_view: CatalogView<'a>,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            catalog_view,
            active_theme,
        }
    }

    #[must_use]
    pub(crate) fn areas(&self, pane: Rect) -> PlaylistAreas {
        let body = pane_block(None, Color::Reset).inner(pane);
        let banner = Rect {
            height: match &self.catalog_view.server.server_status {
                ServerStatus::Offline(_) => body.height.min(1),
                ServerStatus::Connecting | ServerStatus::Online(_) => 0,
            },
            ..body
        };
        let listed = Rect {
            y: body.y.saturating_add(banner.height),
            height: body.height - banner.height,
            ..body
        };
        let scroll_areas = scroll_areas(pane, listed);
        let level = self.catalog_view.level();
        let window = row_window(&WindowFit {
            selected: ViewIndex::new(level.cursor.index()),
            playing_index: None,
            playlist_len: level.catalog_rows.len(),
            height: scroll_areas.content.height,
        });
        let selected_area =
            cursor_band(scroll_areas.rows, window, level.cursor.index());
        PlaylistAreas {
            pane,
            banner,
            scroll_areas,
            window,
            selected_area,
        }
    }

    pub(crate) fn paint(&self, areas: &PlaylistAreas, buffer: &mut Buffer) {
        let pane = areas.pane;
        if pane.width == 0 || pane.height == 0 {
            return;
        }
        let colors = self.active_theme.colors();
        pane_block(
            Some(title_line(pane, self.catalog_view, &self.active_theme)),
            colors.muted_foreground,
        )
        .render(pane, buffer);
        let server = self.catalog_view.server;
        let level = self.catalog_view.level();
        let listed = areas.scroll_areas.content;
        match &server.server_status {
            ServerStatus::Offline(_) => {
                Paragraph::new(offline_banner(server, &colors))
                    .render(areas.banner, buffer);
            }
            ServerStatus::Connecting | ServerStatus::Online(_)
                if level.catalog_rows.is_empty() =>
            {
                match level.paging {
                    Paging::Complete => {
                        let label = match level.listing {
                            Listing::Albums(_) => "No albums",
                            Listing::Album(_) => "No tracks",
                        };
                        Paragraph::new(label)
                            .style(Style::default().fg(colors.foreground))
                            .render(listed, buffer);
                    }
                    Paging::Next(_) | Paging::Loading(_) => {}
                }
            }
            ServerStatus::Connecting | ServerStatus::Online(_) => {}
        }
        if listed.width == 0 || listed.height == 0 || level.catalog_rows.is_empty() {
            return;
        }
        self.paint_rows(areas, buffer);
        paint_scrollbar(
            areas.scroll_areas.scrollbar,
            Scrollbar {
                total: areas.window.playlist_len,
                offset: usize::from(areas.window.offset),
                viewport: usize::from(areas.scroll_areas.scrollbar.height),
                thumb: colors.muted_foreground,
                groove: colors.muted_foreground,
            },
            buffer,
        );
    }

    fn paint_rows(&self, areas: &PlaylistAreas, buffer: &mut Buffer) {
        let colors = self.active_theme.colors();
        let level = self.catalog_view.level();
        let window = areas.window;
        let rows = areas.scroll_areas.rows;
        let row_width = Cells(rows.width);
        let visible = level
            .catalog_rows
            .get(window.start..window.end)
            .unwrap_or(&[]);
        let lines: Vec<Line<'_>> = visible
            .iter()
            .zip(window.start..)
            .map(|(catalog_row, index)| {
                let selected = if index == level.cursor.index() {
                    Selected::Yes
                } else {
                    Selected::No
                };
                match catalog_row {
                    CatalogRow::Album(server_album) => {
                        album_row(server_album, row_width, row_style(selected, &colors))
                    }
                    CatalogRow::Track(track) => track_row_line(
                        &TrackRow {
                            title: track.display(),
                            selected,
                            favorite: self
                                .catalog_view
                                .favorites
                                .favorite(track.source()),
                            playing: self.catalog_view.playing(track),
                            queued_number: None,
                            row_width,
                        },
                        &colors,
                    ),
                }
            })
            .collect();
        let playing_row = visible.iter().position(|catalog_row| match catalog_row {
            CatalogRow::Track(track) => {
                self.catalog_view.playing(track) == Playing::Yes
            }
            CatalogRow::Album(_) => false,
        });
        StatefulWidget::render(
            List::new(lines)
                .highlight_spacing(HighlightSpacing::Never)
                .highlight_style(Style::default().fg(colors.highlight)),
            rows,
            buffer,
            &mut ListState::default().with_selected(playing_row),
        );
        if let Some(band) = areas.selected_area {
            buffer.set_style(band, Style::default().bg(colors.selection_background));
        }
    }
}

fn title_line<'a>(
    area: Rect,
    catalog_view: CatalogView<'a>,
    theme: &ActiveTheme<'_>,
) -> Line<'a> {
    let colors = theme.colors();
    let catalog = catalog_view.catalog;
    let albums_level = &catalog.albums_level;
    let order = match albums_level.listing {
        Listing::Albums(album_order) => Some(label(album_order)),
        Listing::Album(_) => None,
    };
    let album = catalog.album_level.as_ref().and_then(|album_level| {
        match (
            &album_level.listing,
            albums_level.cursor.get(&albums_level.catalog_rows),
        ) {
            (Listing::Album(album_id), Some(CatalogRow::Album(server_album)))
                if server_album.album_id == *album_id =>
            {
                Some(&*server_album.title)
            }
            (
                Listing::Album(_) | Listing::Albums(_),
                Some(CatalogRow::Album(_) | CatalogRow::Track(_)) | None,
            ) => None,
        }
    });
    let level = catalog_view.level();
    let loading = match (level.paging, &level.listing) {
        (Paging::Loading(_), Listing::Albums(_)) => Some("loading albums…"),
        (Paging::Loading(_), Listing::Album(_)) => Some("loading tracks…"),
        (
            Paging::Next(_) | Paging::Complete,
            Listing::Albums(_) | Listing::Album(_),
        ) => None,
    };
    let muted =
        |piece: &'a str| -> StyledText<'a> { text(piece).fg(colors.muted_foreground) };
    let pieces = [
        text(catalog.server_name.as_str()).fg(colors.accent),
        muted(DOT_SEPARATOR),
        text("Albums").fg(colors.foreground),
    ]
    .into_iter()
    .chain(
        order
            .into_iter()
            .flat_map(|order| [muted(": "), text(order).fg(colors.accent)]),
    )
    .chain(
        album
            .into_iter()
            .flat_map(|album| [muted(" › "), text(album).fg(colors.accent)]),
    )
    .chain(
        loading
            .into_iter()
            .flat_map(|loading| [muted(DOT_SEPARATOR), muted(loading)]),
    );
    truncate_line(line(pieces), title_budget(area).count())
}

fn label(album_order: AlbumOrder) -> &'static str {
    match album_order {
        AlbumOrder::Newest => "newest",
        AlbumOrder::Recent => "recent",
        AlbumOrder::Frequent => "frequent",
        AlbumOrder::Starred => "starred",
        AlbumOrder::Alphabetical => "alphabetical",
        AlbumOrder::Random => "random",
    }
}

fn offline_banner<'a>(server: &'a Server, colors: &Colors<Color>) -> Line<'a> {
    line([
        text(OFFLINE_GLYPH).fg(colors.foreground),
        text(" ").fg(colors.foreground),
        text(server.account.endpoint.host()).fg(colors.foreground),
        text(" unreachable").fg(colors.foreground),
        text(DOT_SEPARATOR).fg(colors.muted_foreground),
        text("c opens Servers").fg(colors.muted_foreground),
    ])
}

fn album_row(
    server_album: &ServerAlbum,
    row_width: Cells,
    row_style: Style,
) -> Line<'_> {
    let tracks = match server_album.track_count {
        1 => "track",
        _ => "tracks",
    };
    let summary = format!(
        "{} {tracks}{DOT_SEPARATOR}{}",
        server_album.track_count,
        duration_text(server_album.duration)
    );
    let info = match server_album.year {
        Some(year) => format!("{year}{DOT_SEPARATOR}{summary}"),
        None => summary,
    };
    let markers = usize::from(MARKERS_WIDTH);
    let body_width = row_width.count().saturating_sub(markers);
    let title_width = body_width.saturating_sub(info.width() + GAP);
    let title = truncate_line(
        Line::from(vec![
            Span::raw(&*server_album.artist),
            Span::raw(" — "),
            Span::raw(&*server_album.title),
        ]),
        title_width,
    );
    let fill = body_width.saturating_sub(title.width() + info.width());
    let pieces = once(Span::raw(blanks(markers))).chain(title.spans).chain([
        Span::raw(blanks(fill)),
        Span::styled(info, Style::default().add_modifier(Modifier::DIM)),
    ]);
    truncate_line(Line::from_iter(pieces).style(row_style), row_width.count())
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        appearance::Rgb,
        catalog::{BrowseLevel, Catalog, Paging},
        cursor::Cursor,
        favorites::Favorites,
        geometry::Cells,
        server::{
            Account,
            AlbumId,
            Endpoint,
            Listing,
            RemoteError,
            Server,
            ServerAlbum,
            ServerName,
            ServerStatus,
            ServerTrackId,
            Session,
            UserName,
        },
        track::{CatalogRow, Tags, Track, TrackSource},
    };
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Style},
    };
    use rstest::rstest;

    use crate::{
        playlist::{
            catalog::{CatalogWidget, album_row, title_line},
            view::CatalogView,
        },
        primitive::{canvas::tests::find_text, marker::MARKERS_WIDTH},
        test_support::noir,
        theme::{active_theme::ActiveTheme, colors::Colors, rgb::ColorDepth},
    };

    fn album(album_id: &str, title: &str) -> ServerAlbum {
        ServerAlbum {
            album_id: AlbumId::new(album_id),
            title: Arc::from(title),
            artist: Arc::from("Miles Davis"),
            year: Some(1959),
            track_count: 5,
            duration: Duration::from_secs(2_744),
        }
    }

    fn server_track(title: &str) -> Arc<Track> {
        let source = TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new(title),
        };
        let tags = Tags {
            title: Some(title.to_owned()),
            ..Tags::default()
        };
        Arc::new(Track::tagged(source, Duration::from_secs(545), tags))
    }

    fn albums(count: usize, cursor_index: usize) -> Catalog {
        let mut catalog = Catalog::new(ServerName::new("home"));
        let level = &mut catalog.albums_level;
        level.catalog_rows = (0..count)
            .map(|index| {
                CatalogRow::Album(album(
                    &format!("a-{index}"),
                    &format!("Album {index:02}"),
                ))
            })
            .collect();
        level.cursor = Cursor::at(count, cursor_index);
        level.paging = Paging::Complete;
        catalog
    }

    fn open_album(album_id: &str) -> Catalog {
        Catalog {
            album_level: Some(BrowseLevel {
                listing: Listing::Album(AlbumId::new(album_id)),
                catalog_rows: vec![
                    CatalogRow::Track(server_track("So What")),
                    CatalogRow::Track(server_track("Blue in Green")),
                ],
                cursor: Cursor::at(2, 0),
                paging: Paging::Complete,
            }),
            ..albums(2, 0)
        }
    }

    fn endpoint() -> Endpoint {
        Endpoint::parse("https://music.example.com").unwrap()
    }

    fn online() -> ServerStatus {
        ServerStatus::Online(Session::new(endpoint(), "u=mikey"))
    }

    fn offline() -> ServerStatus {
        ServerStatus::Offline(RemoteError::Moved {
            server_name: ServerName::new("home"),
        })
    }

    fn server(server_status: ServerStatus) -> Server {
        Server {
            account: Account {
                server_name: ServerName::new("home"),
                endpoint: endpoint(),
                user_name: UserName::new("mikey").unwrap(),
            },
            server_status,
        }
    }

    fn painted(
        catalog_view: CatalogView<'_>,
        active_theme: ActiveTheme<'_>,
        pane: Rect,
    ) -> Buffer {
        let widget = CatalogWidget::new(catalog_view, active_theme);
        let areas = widget.areas(pane);
        let mut buffer = Buffer::empty(pane);
        widget.paint(&areas, &mut buffer);
        buffer
    }

    #[rstest]
    #[case::online(online(), Rect::new(2, 1, 35, 0), Rect::new(2, 1, 35, 8))]
    #[case::offline(offline(), Rect::new(2, 1, 35, 1), Rect::new(2, 2, 35, 7))]
    fn the_listed_rows_start_under_the_banner(
        #[case] server_status: ServerStatus,
        #[case] banner: Rect,
        #[case] content: Rect,
    ) {
        let catalog = albums(3, 0);
        let server = server(server_status);
        let favorites = Favorites::default();
        let theme = noir();
        let widget = CatalogWidget::new(
            CatalogView {
                catalog: &catalog,
                server: &server,
                favorites: &favorites,
                playing_track_source: None,
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let areas = widget.areas(Rect::new(0, 0, 40, 10));
        assert_eq!(
            (areas.banner, areas.scroll_areas.content),
            (banner, content)
        );
    }

    #[rstest]
    #[case::the_cursor_album(albums(3, 1), "Album 01", |colors: &Colors<Color>| {
        colors.selection_foreground
    })]
    #[case::another_album(albums(3, 1), "Album 00", |colors: &Colors<Color>| {
        colors.foreground
    })]
    #[case::the_playing_track(open_album("a-0"), "Blue in Green", |colors: &Colors<Color>| {
        colors.highlight
    })]
    #[case::the_cursor_track(open_album("a-0"), "So What", |colors: &Colors<Color>| {
        colors.selection_foreground
    })]
    fn a_catalog_row_wears_the_colour_of_its_state(
        #[case] catalog: Catalog,
        #[case] title: &str,
        #[case] colour: fn(&Colors<Color>) -> Color,
    ) {
        let server = server(online());
        let favorites = Favorites::default();
        let playing = server_track("Blue in Green");
        let mut theme = noir();
        theme.colors.selection_foreground = Rgb([0xff, 0xff, 0xff]);
        let buffer = painted(
            CatalogView {
                catalog: &catalog,
                server: &server,
                favorites: &favorites,
                playing_track_source: Some(playing.source()),
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
            Rect::new(0, 0, 60, 10),
        );
        let (x, y) = find_text(&buffer, title).expect("the row is painted");
        let colors = ActiveTheme::new(&theme, ColorDepth::TrueColor).colors();
        assert_eq!(buffer[(x, y)].style().fg, Some(colour(&colors)));
    }

    #[test]
    fn a_pane_too_narrow_for_rows_paints_the_same_border_with_rows_or_without() {
        let server = server(online());
        let favorites = Favorites::default();
        let theme = noir();
        let pane = Rect::new(0, 0, 2, 10);
        let listed = albums(20, 0);
        let empty = albums(0, 0);
        let buffers = [&listed, &empty].map(|catalog| {
            painted(
                CatalogView {
                    catalog,
                    server: &server,
                    favorites: &favorites,
                    playing_track_source: None,
                },
                ActiveTheme::new(&theme, ColorDepth::TrueColor),
                pane,
            )
        });
        assert_eq!(buffers[0], buffers[1]);
    }

    #[rstest]
    #[case::the_cursor_album_is_open("a-0", "home · Albums: newest › Album 00")]
    #[case::another_album_is_open("a-9", "home · Albums: newest")]
    fn the_title_names_the_open_album_only_under_the_cursor(
        #[case] album_id: &str,
        #[case] expected: &str,
    ) {
        let catalog = open_album(album_id);
        let server = server(online());
        let favorites = Favorites::default();
        let theme = noir();
        let title = title_line(
            Rect::new(0, 0, 80, 10),
            CatalogView {
                catalog: &catalog,
                server: &server,
                favorites: &favorites,
                playing_track_source: None,
            },
            &ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let text: String = title
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, expected);
    }

    #[test]
    fn a_truncated_album_title_keeps_one_blank_before_its_details() {
        let details = "1959 · 5 tracks · 45:44";
        let server_album = album("a-0", "Kind of Blue");
        let row = album_row(&server_album, Cells(MARKERS_WIDTH + 34), Style::default());
        let text: String = row.spans.iter().map(|span| span.content.as_ref()).collect();
        assert!(text.ends_with(&format!(" {details}")), "got {text:?}");
        assert!(!text.ends_with(&format!("  {details}")), "got {text:?}");
    }
}
