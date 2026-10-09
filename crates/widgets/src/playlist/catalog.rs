use std::{iter::once, time::Duration};

use kernel::domain::{
    catalog::{Catalog, Paging},
    geometry::Cells,
    index::ViewIndex,
    overlay::ServerQuery,
    server::{AlbumOrder, Listing, Server, ServerAlbum, ServerPlaylist, ServerStatus},
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
            playlist_len: level.rows().len(),
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
                if level.rows().is_empty() =>
            {
                let label = match (level.query(), level.paging) {
                    (Some(ServerQuery { revision: None, .. }), _) => "Nothing matches",
                    (None, Paging::Complete) => match level.listing {
                        Listing::Songs => "No songs on this server",
                        Listing::Albums(_) => "No albums on this server",
                        Listing::Playlists => "No playlists on this server",
                        Listing::Album(_) | Listing::Playlist(_) => "No tracks",
                    },
                    (Some(_), _)
                    | (
                        None,
                        Paging::Next(_) | Paging::Queued(_) | Paging::Loading(_),
                    ) => "",
                };
                Paragraph::new(label)
                    .style(Style::default().fg(colors.foreground))
                    .render(listed, buffer);
            }
            ServerStatus::Connecting | ServerStatus::Online(_) => {}
        }
        if listed.width == 0 || listed.height == 0 || level.rows().is_empty() {
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
        let visible = level.rows().get(window.start..window.end).unwrap_or(&[]);
        let lines: Vec<Line<'_>> = visible
            .iter()
            .zip(window.start..)
            .map(|(catalog_row, index)| {
                let selected = if index == level.cursor.index() {
                    Selected::Yes
                } else {
                    Selected::No
                };
                let style = row_style(selected, &colors);
                match catalog_row {
                    CatalogRow::Album(server_album) => {
                        album_row(server_album, row_width, style)
                    }
                    CatalogRow::Playlist(server_playlist) => {
                        playlist_row(server_playlist, row_width, style)
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
            CatalogRow::Album(_) | CatalogRow::Playlist(_) => false,
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
    let (view, order) = match albums_level.listing {
        Listing::Songs => ("Songs", None),
        Listing::Albums(album_order) => ("Albums", Some(label(album_order))),
        Listing::Album(_) => ("Albums", None),
        Listing::Playlists | Listing::Playlist(_) => ("Playlists", None),
    };
    let album = open_title(catalog);
    let level = catalog_view.level();
    let searching = level
        .server_query
        .as_ref()
        .and_then(|server_query| server_query.revision);
    let loading = searching
        .map(|_revision| "searching…")
        .or(match level.paging {
            Paging::Queued(_) | Paging::Loading(_) => Some(match level.listing {
                Listing::Songs => "loading songs…",
                Listing::Albums(_) => "loading albums…",
                Listing::Playlists => "loading playlists…",
                Listing::Album(_) | Listing::Playlist(_) => "loading tracks…",
            }),
            Paging::Next(_) | Paging::Complete => None,
        });
    let filter = level.query().map(|server_query| {
        let all = level.catalog_rows.len();
        format!("{} / {all} · /{}", level.rows().len(), server_query.input)
    });
    let muted =
        |piece: &'a str| -> StyledText<'a> { text(piece).fg(colors.muted_foreground) };
    let pieces =
        [
            text(catalog.server_name.as_str()).fg(colors.accent),
            muted(DOT_SEPARATOR),
            text(view).fg(colors.foreground),
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
        .chain(filter.into_iter().flat_map(|filter| {
            [muted(DOT_SEPARATOR), text(filter).fg(colors.foreground)]
        }))
        .chain(loading.into_iter().flat_map(|loading| {
            let [glyph, gap] = theme.spinner.mark(&colors);
            [muted(DOT_SEPARATOR), glyph, gap, muted(loading)]
        }));
    truncate_line(line(pieces), title_budget(area).count())
}

fn open_title(catalog: &Catalog) -> Option<&str> {
    catalog.album_level.as_ref().and_then(|album_level| {
        match (
            &album_level.listing,
            catalog.albums_level.cursor.get(catalog.albums_level.rows()),
        ) {
            (Listing::Album(album_id), Some(CatalogRow::Album(server_album)))
                if server_album.album_id == *album_id =>
            {
                Some(&*server_album.title)
            }
            (
                Listing::Playlist(playlist_id),
                Some(CatalogRow::Playlist(server_playlist)),
            ) if server_playlist.playlist_id == *playlist_id => {
                Some(&*server_playlist.name)
            }
            (
                Listing::Album(_)
                | Listing::Albums(_)
                | Listing::Songs
                | Listing::Playlists
                | Listing::Playlist(_),
                Some(
                    CatalogRow::Album(_)
                    | CatalogRow::Playlist(_)
                    | CatalogRow::Track(_),
                )
                | None,
            ) => None,
        }
    })
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
    let summary = summary(server_album.track_count, server_album.duration);
    let info = match server_album.year {
        Some(year) => format!("{year}{DOT_SEPARATOR}{summary}"),
        None => summary,
    };
    let title = Line::from(vec![
        Span::raw(&*server_album.artist),
        Span::raw(" — "),
        Span::raw(&*server_album.title),
    ]);
    summary_row(title, info, row_width).style(row_style)
}

fn playlist_row(
    server_playlist: &ServerPlaylist,
    row_width: Cells,
    row_style: Style,
) -> Line<'_> {
    let summary = summary(server_playlist.track_count, server_playlist.duration);
    summary_row(Line::from(&*server_playlist.name), summary, row_width).style(row_style)
}

fn summary(track_count: usize, duration: Duration) -> String {
    let tracks = match track_count {
        1 => "track",
        _ => "tracks",
    };
    format!(
        "{track_count} {tracks}{DOT_SEPARATOR}{}",
        duration_text(duration)
    )
}

fn summary_row<'a>(title: Line<'a>, summary: String, row_width: Cells) -> Line<'a> {
    let markers = usize::from(MARKERS_WIDTH);
    let body_width = row_width.count().saturating_sub(markers);
    let title = truncate_line(title, body_width.saturating_sub(summary.width() + GAP));
    let fill = body_width.saturating_sub(title.width() + summary.width());
    let pieces = once(Span::raw(blanks(markers))).chain(title.spans).chain([
        Span::raw(blanks(fill)),
        Span::styled(summary, Style::default().add_modifier(Modifier::DIM)),
    ]);
    truncate_line(Line::from_iter(pieces), row_width.count())
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
        overlay::ServerQuery,
        server::{
            Account,
            AlbumId,
            AlbumOrder,
            Endpoint,
            Listing,
            PlaylistId,
            RemoteError,
            Server,
            ServerAlbum,
            ServerName,
            ServerPlaylist,
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
        level.listing = Listing::Albums(AlbumOrder::Newest);
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
                catalog_rows: vec![
                    CatalogRow::Track(server_track("So What")),
                    CatalogRow::Track(server_track("Blue in Green")),
                ],
                cursor: Cursor::at(2, 0),
                paging: Paging::Complete,
                ..BrowseLevel::new(Listing::Album(AlbumId::new(album_id)))
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
    #[case::the_cursor_album_is_open(
        open_album("a-0"),
        "home · Albums: newest › Album 00"
    )]
    #[case::another_album_is_open(open_album("a-9"), "home · Albums: newest")]
    #[case::an_empty_filter_is_open(Catalog { albums_level: BrowseLevel { server_query: Some(ServerQuery::default()), ..albums(2, 0).albums_level }, ..albums(2, 0) }, "home · Albums: newest")]
    fn the_title_names_the_open_album_only_under_the_cursor(
        #[case] catalog: Catalog,
        #[case] expected: &str,
    ) {
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
        assert_eq!(title.to_string(), expected);
    }

    #[test]
    fn a_songs_view_names_songs_in_the_title_and_lists_tracks() {
        let mut catalog = Catalog::new(ServerName::new("home"));
        catalog.albums_level.catalog_rows = vec![
            CatalogRow::Track(server_track("So What")),
            CatalogRow::Track(server_track("Blue in Green")),
        ];
        catalog.albums_level.cursor = Cursor::at(2, 0);
        let server = server(online());
        let favorites = Favorites::default();
        let theme = noir();
        let catalog_view = CatalogView {
            catalog: &catalog,
            server: &server,
            favorites: &favorites,
            playing_track_source: None,
        };
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let title = title_line(Rect::new(0, 0, 80, 10), catalog_view, &active_theme);
        let text: String = title
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "home · Songs");
        let buffer = painted(catalog_view, active_theme, Rect::new(0, 0, 40, 6));
        let rows: String = buffer
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(rows.contains("So What"), "{rows}");
        assert!(rows.contains("Blue in Green"), "{rows}");
    }

    #[test]
    fn a_songs_view_paints_its_rows_like_local_playlist_rows() {
        let song = |title: &str, artist: &str| {
            let source = TrackSource::Server {
                server_name: ServerName::new("home"),
                server_track_id: ServerTrackId::new(title),
            };
            let tags = Tags {
                title: Some(title.to_owned()),
                artist: Some(artist.to_owned()),
                ..Tags::default()
            };
            CatalogRow::Track(Arc::new(Track::tagged(
                source,
                Duration::from_secs(545),
                tags,
            )))
        };
        let mut catalog = Catalog::new(ServerName::new("home"));
        catalog.albums_level.catalog_rows = vec![
            song("So What", "Miles Davis"),
            song("Blue in Green", "Bill Evans"),
        ];
        catalog.albums_level.cursor = Cursor::at(2, 0);
        let server = server(online());
        let favorites = Favorites::default();
        let theme = noir();
        let pane = Rect::new(0, 0, 50, 6);
        let buffer = painted(
            CatalogView {
                catalog: &catalog,
                server: &server,
                favorites: &favorites,
                playing_track_source: None,
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
            pane,
        );
        let lines: Vec<String> = (pane.top()..pane.bottom())
            .map(|y| {
                (pane.left()..pane.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        insta::assert_snapshot!(lines.join("\n"));
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

    fn playlists() -> Catalog {
        let mut catalog = Catalog::new(ServerName::new("home"));
        let level = &mut catalog.albums_level;
        level.listing = Listing::Playlists;
        level.catalog_rows = [
            ("pl-0", "Late Night", 12, 3_120),
            ("pl-1", "Morning", 1, 240),
        ]
        .map(|(playlist_id, name, track_count, seconds)| {
            CatalogRow::Playlist(ServerPlaylist {
                playlist_id: PlaylistId::new(playlist_id),
                name: Arc::from(name),
                track_count,
                duration: Duration::from_secs(seconds),
            })
        })
        .into();
        level.cursor = Cursor::at(2, 0);
        level.paging = Paging::Complete;
        catalog
    }

    fn painted_lines(catalog: &Catalog) -> String {
        let server = server(online());
        let favorites = Favorites::default();
        let theme = noir();
        let pane = Rect::new(0, 0, 50, 6);
        let buffer = painted(
            CatalogView {
                catalog,
                server: &server,
                favorites: &favorites,
                playing_track_source: None,
            },
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
            pane,
        );
        let lines: Vec<String> = (pane.top()..pane.bottom())
            .map(|y| {
                (pane.left()..pane.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        lines.join("\n")
    }

    #[test]
    fn a_playlists_view_paints_each_playlist_with_its_track_count_and_duration() {
        insta::assert_snapshot!(painted_lines(&playlists()));
    }

    #[test]
    fn an_open_playlist_is_named_in_the_title_and_lists_its_tracks() {
        let catalog = Catalog {
            album_level: Some(BrowseLevel {
                listing: Listing::Playlist(PlaylistId::new("pl-0")),
                catalog_rows: vec![
                    CatalogRow::Track(server_track("So What")),
                    CatalogRow::Track(server_track("Blue in Green")),
                ],
                cursor: Cursor::at(2, 1),
                paging: Paging::Complete,
                server_query: None,
            }),
            ..playlists()
        };
        insta::assert_snapshot!(painted_lines(&catalog));
    }
}
