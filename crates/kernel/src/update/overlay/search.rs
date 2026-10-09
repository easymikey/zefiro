use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        catalog::{BrowseLevel, Catalog, CatalogName},
        cursor::Cursor,
        cursor_over::CursorOver,
        index::ViewIndex,
        overlay::{SearchQuery, ServerQuery},
        playlist::PlaylistRows,
        server::{Listing, Server, ServerName, Session},
        track::CatalogRow,
    },
    message::{Message, QueueRequest, SearchRequest, TextRequest},
    update::{
        machine::{Machine, Unhandled, replace},
        overlay::{OverlayParts, text_entry::edit},
        player::events::session,
    },
};

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchRequest;
    type Effect = Cmd;

    fn transition(&mut self, message: SearchRequest) -> Result<Cmd, Unhandled> {
        match message {
            SearchRequest::Edit(text_request) => {
                edit(&mut self.content.input, text_request)?;
                Ok(Cmd::none())
            }
            SearchRequest::Navigate(direction) => {
                let moved = self.cursor.step(direction.sign());
                replace(&mut self.cursor, moved).map(|()| Cmd::none())
            }
            SearchRequest::Enqueue => enqueue(self),
        }
    }
}

pub(crate) fn server_request(
    parts: &mut OverlayParts<'_>,
    request: SearchRequest,
) -> Result<Cmd, Unhandled> {
    let OverlayParts {
        workspace: _,
        rows: _,
        player: _,
        history: _,
        music_dir: _,
        servers,
        catalog_name,
        catalogs,
        revisions,
    } = parts;
    let CatalogName::Server(server_name) = catalog_name else {
        return Err(Unhandled);
    };
    let level = level(catalogs, catalog_name).ok_or(Unhandled)?;
    match request {
        SearchRequest::Edit(text_request) => {
            let session = listing_session(&level.listing, servers, server_name)?;
            let mut input = level
                .server_query
                .as_ref()
                .map_or_else(String::new, |server_query| server_query.input.clone());
            edit(&mut input, text_request)?;
            let needle = input.to_lowercase();
            let server_query = level.server_query.get_or_insert_default();
            server_query.input = input;
            server_query.revision = None;
            let cmd = match session {
                Some(session) if !needle.is_empty() => {
                    let revision = revisions.issue_effect();
                    server_query.revision = Some(revision);
                    Effect::Remote(RemoteCmd::Search {
                        server_name: server_name.clone(),
                        session,
                        input: server_query.input.clone(),
                        listing: level.listing.clone(),
                        revision,
                    })
                    .into()
                }
                None if !needle.is_empty() => {
                    narrowed(level);
                    Cmd::none()
                }
                Some(_) | None => {
                    server_query.catalog_rows.clear();
                    Cmd::none()
                }
            };
            level.cursor = Cursor::new(level.rows().len());
            Ok(cmd)
        }
        SearchRequest::Navigate(direction) => {
            let moved = level.cursor.step(direction.sign());
            replace(&mut level.cursor, moved).map(|()| Cmd::none())
        }
        SearchRequest::Enqueue => Err(Unhandled),
    }
}

pub(crate) fn clear(parts: &mut OverlayParts<'_>) -> Option<ServerQuery> {
    let level = level(parts.catalogs, parts.catalog_name)?;
    let server_query = level.server_query.take()?;
    level.cursor = Cursor::new(level.catalog_rows.len());
    Some(server_query)
}

pub(crate) fn level<'a>(
    catalogs: &'a mut [Catalog],
    catalog_name: &CatalogName,
) -> Option<&'a mut BrowseLevel> {
    match catalog_name {
        CatalogName::Local => None,
        CatalogName::Server(server_name) => catalogs
            .iter_mut()
            .find(|catalog| catalog.server_name == *server_name)
            .map(Catalog::level),
    }
}

fn listing_session(
    listing: &Listing,
    servers: &[Server],
    server_name: &ServerName,
) -> Result<Option<Session>, Unhandled> {
    match listing {
        Listing::Songs | Listing::Albums(_) => session(servers, server_name)
            .cloned()
            .map(Some)
            .ok_or(Unhandled),
        Listing::Album(_) | Listing::Playlists | Listing::Playlist(_) => Ok(None),
    }
}

pub(crate) fn narrowed(level: &mut BrowseLevel) {
    if let Listing::Album(_) | Listing::Playlists | Listing::Playlist(_) = level.listing
        && let Some(server_query) = level
            .server_query
            .as_mut()
            .filter(|server_query| !server_query.input.is_empty())
    {
        let needle = server_query.input.to_lowercase();
        server_query.catalog_rows = level
            .catalog_rows
            .iter()
            .filter(|catalog_row| matched(catalog_row, &needle))
            .cloned()
            .collect();
    }
}

fn matched(catalog_row: &CatalogRow, needle: &str) -> bool {
    match catalog_row {
        CatalogRow::Track(track) => track.display().to_lowercase().contains(needle),
        CatalogRow::Album(server_album) => [&server_album.title, &server_album.artist]
            .iter()
            .any(|text| text.to_lowercase().contains(needle)),
        CatalogRow::Playlist(server_playlist) => {
            server_playlist.name.to_lowercase().contains(needle)
        }
    }
}

pub(crate) fn rerank(
    search_query: &mut CursorOver<SearchQuery>,
    rows: PlaylistRows<'_>,
) {
    let ranked = crate::search::rank(rows, &search_query.content.input);
    refreshed(search_query, ranked);
}

pub(crate) fn requery(
    search_query: &mut CursorOver<SearchQuery>,
    rows: PlaylistRows<'_>,
    text_request: TextRequest,
) {
    let input = &search_query.content.input;
    let ranked = match text_request {
        TextRequest::Char(_) => {
            crate::search::narrow(rows, input, &search_query.content.matches)
        }
        TextRequest::Backspace | TextRequest::DeleteWord | TextRequest::Clear => {
            crate::search::rank(rows, input)
        }
    };
    refreshed(search_query, ranked);
}

fn refreshed(search_query: &mut CursorOver<SearchQuery>, matches: Vec<ViewIndex>) {
    search_query.cursor = Cursor::new(matches.len());
    search_query.content.matches = matches;
}

impl CursorOver<SearchQuery> {
    #[must_use]
    pub(crate) fn selected_match(&self) -> Option<ViewIndex> {
        self.content.matches.get(self.selected().get()).copied()
    }
}

fn enqueue(search_query: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search_query.selected_match().ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Queue(QueueRequest::ToggleAt(index))))
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use crate::{
        cmd::{Effect, RemoteCmd},
        domain::{
            catalog::{BrowseLevel, Catalog, CatalogName, Paging},
            cursor::Cursor,
            favorites::Favorites,
            key::{Key, KeyCode, KeyPress},
            model::Model,
            overlay::Overlay,
            revision::Revision,
            server::{
                Account,
                AlbumId,
                AlbumOrder,
                Endpoint,
                Listing,
                Page,
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
            time::Moment,
            track::{CatalogRow, Track, TrackSource},
        },
        message::{CatalogPage, Message, RemoteEvent},
        update::{machine::Unhandled, update},
    };

    fn press(model: &mut Model, code: KeyCode) -> Result<Vec<Effect>, Unhandled> {
        let key = Key::plain(code);
        update(
            model,
            Message::Key(KeyPress { key, typed: key }),
            Moment::default(),
        )
    }

    fn home() -> ServerName {
        ServerName::new("home")
    }

    fn home_server(server_status: ServerStatus) -> Server {
        let endpoint = Endpoint::parse("https://home.example.com").unwrap();
        Server {
            account: Account {
                server_name: home(),
                endpoint,
                user_name: UserName::new("ann").unwrap(),
            },
            server_status,
        }
    }

    fn online() -> ServerStatus {
        let endpoint = Endpoint::parse("https://home.example.com").unwrap();
        ServerStatus::Online(Session::new(endpoint, "u=ann&t=t&s=s"))
    }

    fn home_tab(server_status: ServerStatus, albums_level: BrowseLevel) -> Model {
        let mut model = Model {
            servers: vec![home_server(server_status)],
            catalog_name: CatalogName::Server(home()),
            ..Model::default()
        };
        model.catalogs = vec![Catalog {
            albums_level,
            ..Catalog::new(home())
        }];
        model
    }

    fn listed(listing: Listing, catalog_rows: Vec<CatalogRow>) -> BrowseLevel {
        BrowseLevel {
            cursor: Cursor::new(catalog_rows.len()),
            catalog_rows,
            paging: Paging::Complete,
            ..BrowseLevel::new(listing)
        }
    }

    fn playlists() -> BrowseLevel {
        let playlist_row = |name: &str| {
            CatalogRow::Playlist(ServerPlaylist {
                playlist_id: PlaylistId::new(name),
                name: Arc::from(name),
                track_count: 1,
                duration: Duration::from_secs(60),
            })
        };
        listed(
            Listing::Playlists,
            vec![
                playlist_row("Jazz"),
                playlist_row("Rock"),
                playlist_row("Jazz Live"),
            ],
        )
    }

    fn songs() -> BrowseLevel {
        listed(Listing::Songs, vec![track_row("a"), track_row("b")])
    }

    fn track_row(id: &str) -> CatalogRow {
        CatalogRow::Track(Arc::new(Track::from(TrackSource::Server {
            server_name: home(),
            server_track_id: ServerTrackId::new(id),
        })))
    }

    fn level(model: &Model) -> &BrowseLevel {
        &model.catalogs.first().unwrap().albums_level
    }

    fn names(model: &Model) -> Vec<&str> {
        level(model)
            .rows()
            .iter()
            .filter_map(|catalog_row| match catalog_row {
                CatalogRow::Playlist(server_playlist) => Some(&*server_playlist.name),
                CatalogRow::Album(_) | CatalogRow::Track(_) => None,
            })
            .collect()
    }

    fn typed(model: &mut Model, text: &str) -> Vec<(String, Listing, Revision)> {
        text.chars()
            .flat_map(|character| press(model, KeyCode::Char(character)).unwrap())
            .filter_map(|effect| {
                if let Effect::Remote(RemoteCmd::Search {
                    input,
                    listing,
                    revision,
                    ..
                }) = effect
                {
                    Some((input, listing, revision))
                } else {
                    None
                }
            })
            .collect()
    }

    fn found(
        model: &mut Model,
        catalog_rows: Vec<CatalogRow>,
        revision: Revision,
    ) -> Result<Vec<Effect>, Unhandled> {
        update(
            model,
            Message::Remote(RemoteEvent::Found {
                server_name: home(),
                result: Ok((catalog_rows, Favorites::default())),
                revision,
            }),
            Moment::default(),
        )
    }

    #[test]
    fn slash_in_a_playlists_view_narrows_its_rows_locally_as_the_user_types() {
        let mut model = home_tab(ServerStatus::Connecting, playlists());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        assert_eq!(model.workspace.overlay, Some(Overlay::ServerSearch));
        assert_eq!(typed(&mut model, "ja"), Vec::new());
        assert_eq!(names(&model), ["Jazz", "Jazz Live"]);
        assert_eq!(level(&model).catalog_rows.len(), 3);
    }

    #[test]
    fn typing_in_a_songs_view_asks_the_server_for_songs_and_drops_a_stale_answer() {
        let mut model = home_tab(online(), songs());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        let sent = typed(&mut model, "so");
        let [
            (first, Listing::Songs, stale),
            (second, Listing::Songs, current),
        ] = sent.as_slice()
        else {
            panic!("two song searches, got {sent:?}");
        };
        assert_eq!((first.as_str(), second.as_str()), ("s", "so"));
        assert!(found(&mut model, vec![track_row("stale")], *stale).is_err());
        assert!(found(&mut model, vec![track_row("so-what")], *current).is_ok());
        assert_eq!(level(&model).rows(), [track_row("so-what")]);
        assert_eq!(level(&model).catalog_rows.len(), 2);
    }

    #[test]
    fn a_songs_filter_without_a_session_is_refused_and_keeps_no_filter() {
        let mut model = home_tab(ServerStatus::Connecting, songs());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        assert!(press(&mut model, KeyCode::Char('s')).is_err());
        assert_eq!(level(&model).server_query, None);
    }

    #[test]
    fn emptying_the_input_shows_every_row_and_sends_no_search() {
        let mut model = home_tab(online(), songs());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        assert_eq!(typed(&mut model, "s").len(), 1);
        let effects = press(&mut model, KeyCode::Backspace).unwrap();
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                Effect::Remote(RemoteCmd::Search { .. })
            ))
        );
        assert_eq!(level(&model).rows().len(), 2);
    }

    #[test]
    fn enter_keeps_the_filter_over_the_rows_and_esc_clears_it() {
        let mut model = home_tab(ServerStatus::Connecting, playlists());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        typed(&mut model, "rock");
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        assert_eq!(model.workspace.overlay, None);
        assert_eq!(names(&model), ["Rock"]);
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        assert!(press(&mut model, KeyCode::Esc).is_ok());
        assert_eq!(model.workspace.overlay, None);
        assert_eq!(level(&model).server_query, None);
        assert_eq!(names(&model), ["Jazz", "Rock", "Jazz Live"]);
    }

    #[test]
    fn esc_on_the_rows_clears_a_kept_filter_at_once() {
        let mut model = home_tab(ServerStatus::Connecting, playlists());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        typed(&mut model, "rock");
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        assert!(press(&mut model, KeyCode::Esc).is_ok());
        assert_eq!(level(&model).server_query, None);
        assert_eq!(names(&model), ["Jazz", "Rock", "Jazz Live"]);
        assert!(press(&mut model, KeyCode::Esc).is_err());
    }

    #[test]
    fn a_failed_search_toasts_and_drops_any_later_answer_to_it() {
        let mut model = home_tab(online(), songs());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        let sent = typed(&mut model, "s");
        let [(_, Listing::Songs, revision)] = sent.as_slice() else {
            panic!("one song search, got {sent:?}");
        };
        let failed = update(
            &mut model,
            Message::Remote(RemoteEvent::Found {
                server_name: home(),
                result: Err(RemoteError::Moved {
                    server_name: home(),
                }),
                revision: *revision,
            }),
            Moment::default(),
        );
        assert!(failed.is_ok());
        assert_eq!(model.workspace.toasts.len(), 1);
        assert!(found(&mut model, vec![track_row("late")], *revision).is_err());
        assert_eq!(level(&model).rows(), []);
    }

    #[test]
    fn tab_while_filtering_is_refused_and_keeps_the_view_and_the_filter() {
        let mut model = home_tab(ServerStatus::Connecting, playlists());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        typed(&mut model, "ja");
        assert!(press(&mut model, KeyCode::Tab).is_err());
        assert_eq!(model.workspace.overlay, Some(Overlay::ServerSearch));
        assert_eq!(model.catalog_name, CatalogName::Server(home()));
        assert_eq!(level(&model).listing, Listing::Playlists);
        assert_eq!(names(&model), ["Jazz", "Jazz Live"]);
    }

    #[test]
    fn enter_on_a_filtered_song_plays_the_matched_track() {
        let mut model = home_tab(online(), songs());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        let sent = typed(&mut model, "so");
        let Some((_, _, current)) = sent.last() else {
            panic!("song searches, got {sent:?}");
        };
        assert!(found(&mut model, vec![track_row("so-what")], *current).is_ok());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        let played: Vec<_> = model
            .playlist
            .tracks
            .iter()
            .map(|track| track.source())
            .collect();
        assert_eq!(
            played,
            [&TrackSource::Server {
                server_name: home(),
                server_track_id: ServerTrackId::new("so-what"),
            }]
        );
    }

    #[test]
    fn enter_on_a_filtered_album_opens_the_matched_album() {
        let album_row = |id: &str| {
            CatalogRow::Album(ServerAlbum {
                album_id: AlbumId::new(id),
                title: Arc::from(id),
                artist: Arc::from("Miles"),
                year: None,
                track_count: 1,
                duration: Duration::from_secs(60),
            })
        };
        let albums = listed(
            Listing::Albums(AlbumOrder::Newest),
            vec![album_row("Bags"), album_row("Kind of Blue")],
        );
        let mut model = home_tab(online(), albums);
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        let sent = typed(&mut model, "kind");
        let Some((_, Listing::Albums(_), current)) = sent.last() else {
            panic!("album searches, got {sent:?}");
        };
        assert!(found(&mut model, vec![album_row("Kind of Blue")], *current).is_ok());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        let opened = model
            .catalogs
            .first()
            .unwrap()
            .album_level
            .as_ref()
            .unwrap();
        assert_eq!(opened.listing, Listing::Album(AlbumId::new("Kind of Blue")));
    }

    #[test]
    fn enter_on_a_filtered_playlist_opens_the_matched_playlist() {
        let mut model = home_tab(online(), playlists());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        assert_eq!(typed(&mut model, "rock"), Vec::new());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        let opened = model
            .catalogs
            .first()
            .unwrap()
            .album_level
            .as_ref()
            .unwrap();
        assert_eq!(opened.listing, Listing::Playlist(PlaylistId::new("Rock")));
    }

    #[test]
    fn a_filter_typed_before_the_page_lands_narrows_the_rows_that_arrive() {
        let browse_level = BrowseLevel {
            paging: Paging::Loading(Page::default()),
            ..BrowseLevel::new(Listing::Playlists)
        };
        let mut model = home_tab(ServerStatus::Connecting, browse_level);
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        assert_eq!(typed(&mut model, "ja"), Vec::new());
        let revision = model.revisions.list;
        let arrived = update(
            &mut model,
            Message::Remote(RemoteEvent::Listed(CatalogPage {
                server_name: home(),
                listing: Listing::Playlists,
                page: Page::default(),
                catalog_rows: playlists().catalog_rows,
                favorites: Favorites::default(),
                revision,
            })),
            Moment::default(),
        );
        assert!(arrived.is_ok());
        assert_eq!(names(&model), ["Jazz", "Jazz Live"]);
        assert_eq!(level(&model).catalog_rows.len(), 3);
    }

    #[test]
    fn enter_on_an_emptied_filter_leaves_no_filter() {
        let mut model = home_tab(ServerStatus::Connecting, playlists());
        assert!(press(&mut model, KeyCode::Char('/')).is_ok());
        typed(&mut model, "r");
        assert!(press(&mut model, KeyCode::Backspace).is_ok());
        assert!(press(&mut model, KeyCode::Enter).is_ok());
        assert_eq!(model.workspace.overlay, None);
        assert_eq!(level(&model).server_query, None);
        assert_eq!(names(&model), ["Jazz", "Rock", "Jazz Live"]);
    }
}
