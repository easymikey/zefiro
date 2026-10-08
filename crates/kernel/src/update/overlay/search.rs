use std::sync::Arc;

use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        index::ViewIndex,
        overlay::{Overlay, SearchQuery, ServerQuery},
        revision::Revision,
        server::RemoteError,
        toast::Toast,
        track::{CatalogRow, Track},
    },
    message::{Message, QueueRequest, SearchEdit, SearchRequest},
    update::{
        machine::{Machine, Unhandled, replace},
        overlay::OverlayParts,
        player::events::session,
    },
};

impl Machine for CursorOver<SearchQuery> {
    type Message = SearchRequest;
    type Effect = Cmd;

    fn transition(&mut self, message: SearchRequest) -> Result<Cmd, Unhandled> {
        match message {
            SearchRequest::Edit(edit) => {
                edit_query(&mut self.content.input, edit)?;
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
        workspace,
        playlist: _playlist,
        player: _player,
        history: _history,
        music_dir: _music_dir,
        servers,
        catalog_name: _catalog_name,
        revisions,
    } = parts;
    let Some(Overlay::ServerSearch(server_query)) = workspace.overlay.as_mut() else {
        return Err(Unhandled);
    };
    match request {
        SearchRequest::Edit(edit) => {
            let ServerQuery {
                server_name,
                input,
                catalog_rows,
                revision,
            } = &mut server_query.content;
            let session = session(servers, server_name).cloned().ok_or(Unhandled)?;
            edit_query(input, edit)?;
            if input.is_empty() {
                catalog_rows.clear();
                *revision = None;
                server_query.cursor = Cursor::new(0);
                return Ok(Cmd::none());
            }
            let issued = revisions.issue_effect();
            *revision = Some(issued);
            Ok(Cmd::from(Effect::Remote(RemoteCmd::Search {
                server_name: server_name.clone(),
                session,
                input: input.clone(),
                revision: issued,
            })))
        }
        SearchRequest::Navigate(direction) => {
            let moved = server_query.cursor.step(direction.sign());
            replace(&mut server_query.cursor, moved).map(|()| Cmd::none())
        }
        SearchRequest::Enqueue => Err(Unhandled),
    }
}

pub(crate) fn found(
    overlay: &mut Option<Overlay>,
    result: Result<Vec<CatalogRow>, RemoteError>,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    let Some(Overlay::ServerSearch(server_query)) = overlay.as_mut() else {
        return Err(Unhandled);
    };
    server_query
        .content
        .revision
        .take_if(|sent| *sent == revision)
        .map(drop)
        .ok_or(Unhandled)?;
    Ok(match result {
        Ok(catalog_rows) => {
            server_query.cursor = Cursor::new(catalog_rows.len());
            server_query.content.catalog_rows = catalog_rows;
            Cmd::none()
        }
        Err(error) => Cmd::message(Message::Toast(Toast::error(error.to_string()))),
    })
}

pub(crate) fn rerank(
    search_query: &mut CursorOver<SearchQuery>,
    tracks: &[Arc<Track>],
) {
    let ranked = crate::search::rank(tracks, &search_query.content.input);
    refreshed(search_query, ranked);
}

pub(crate) fn requery(
    search_query: &mut CursorOver<SearchQuery>,
    tracks: &[Arc<Track>],
    edit: SearchEdit,
) {
    let input = &search_query.content.input;
    let ranked = match edit {
        SearchEdit::Char(_) => {
            crate::search::narrow(tracks, input, &search_query.content.matches)
        }
        SearchEdit::Backspace | SearchEdit::DeleteWord | SearchEdit::Clear => {
            crate::search::rank(tracks, input)
        }
    };
    refreshed(search_query, ranked);
}

fn refreshed(search_query: &mut CursorOver<SearchQuery>, matches: Vec<ViewIndex>) {
    search_query.cursor = Cursor::new(matches.len());
    search_query.content.matches = matches;
}

fn edit_query(input: &mut String, edit: SearchEdit) -> Result<(), Unhandled> {
    match edit {
        SearchEdit::Backspace | SearchEdit::DeleteWord | SearchEdit::Clear
            if input.is_empty() =>
        {
            return Err(Unhandled);
        }
        SearchEdit::Char(character) => input.push(character),
        SearchEdit::Backspace => {
            input.pop();
        }
        SearchEdit::DeleteWord => delete_trailing_word(input),
        SearchEdit::Clear => input.clear(),
    }
    Ok(())
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

fn delete_trailing_word(input: &mut String) {
    let trimmed = input.trim_end();
    let cut = trimmed
        .rfind(char::is_whitespace)
        .map_or(0, |index| index + 1);
    input.truncate(cut);
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use crate::{
        cmd::{Effect, RemoteCmd},
        domain::{
            catalog::{Catalog, CatalogName},
            cursor::Cursor,
            cursor_over::CursorOver,
            favorites::Favorites,
            key::{Key, KeyCode, KeyPress},
            model::Model,
            overlay::{Overlay, ServerQuery},
            playlist::PlaylistSource,
            revision::Revision,
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
            time::Moment,
            toast::ToastLevel,
            track::{CatalogRow, Track, TrackSource},
        },
        message::{Message, RemoteEvent},
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

    fn model_with(servers: Vec<Server>, overlay: Option<Overlay>) -> Model {
        let mut model = Model {
            servers,
            ..Model::default()
        };
        model.workspace.overlay = overlay;
        model
    }

    fn home() -> ServerName {
        ServerName::new("home")
    }

    fn online_home() -> Server {
        let endpoint = Endpoint::parse("https://home.example.com").unwrap();
        Server {
            account: Account {
                server_name: home(),
                endpoint: endpoint.clone(),
                user_name: UserName::new("ann").unwrap(),
            },
            server_status: ServerStatus::Online(Session::new(
                endpoint,
                "u=ann&t=t&s=s",
            )),
        }
    }

    fn home_tab(server: Server, overlay: Option<Overlay>) -> Model {
        let mut model = model_with(vec![server], overlay);
        model.catalog_name = CatalogName::Server(home());
        model.catalogs = vec![Catalog::new(home())];
        model
    }

    fn server_query(
        catalog_rows: Vec<CatalogRow>,
        revision: Option<Revision>,
    ) -> CursorOver<ServerQuery> {
        let len = catalog_rows.len();
        CursorOver::new(
            ServerQuery {
                server_name: home(),
                input: String::new(),
                catalog_rows,
                revision,
            },
            len,
        )
    }

    fn album_row(id: &str) -> CatalogRow {
        CatalogRow::Album(ServerAlbum {
            album_id: AlbumId::new(id),
            title: Arc::from(id),
            artist: Arc::from("artist"),
            year: None,
            track_count: 1,
            duration: Duration::from_secs(60),
        })
    }

    fn track_row(id: &str) -> CatalogRow {
        CatalogRow::Track(Arc::new(Track::from(TrackSource::Server {
            server_name: home(),
            server_track_id: ServerTrackId::new(id),
        })))
    }

    fn sent_revisions(effects: &[Effect]) -> Vec<(String, Revision)> {
        effects
            .iter()
            .filter_map(|effect| {
                if let Effect::Remote(RemoteCmd::Search {
                    server_name: _server_name,
                    session: _session,
                    input,
                    revision,
                }) = effect
                {
                    Some((input.clone(), *revision))
                } else {
                    None
                }
            })
            .collect()
    }

    fn found(
        model: &mut Model,
        result: Result<Vec<CatalogRow>, RemoteError>,
        revision: Revision,
    ) -> Result<Vec<Effect>, Unhandled> {
        update(
            model,
            Message::Remote(RemoteEvent::Found {
                server_name: home(),
                result: result.map(|catalog_rows| (catalog_rows, Favorites::default())),
                revision,
            }),
            Moment::default(),
        )
    }

    fn opened_query(model: &Model) -> &CursorOver<ServerQuery> {
        let Some(Overlay::ServerSearch(server_query)) = &model.workspace.overlay else {
            panic!(
                "expected the server search, got {:?}",
                model.workspace.overlay
            );
        };
        server_query
    }

    #[test]
    fn slash_in_the_local_tab_opens_the_local_search() {
        let mut model = model_with(vec![online_home()], None);

        press(&mut model, KeyCode::Char('/')).unwrap();

        assert!(matches!(model.workspace.overlay, Some(Overlay::Search(_))));
    }

    #[test]
    fn slash_in_an_online_server_tab_opens_the_server_search() {
        let mut model = home_tab(online_home(), None);

        press(&mut model, KeyCode::Char('/')).unwrap();

        assert_eq!(
            model.workspace.overlay,
            Some(Overlay::ServerSearch(server_query(Vec::new(), None)))
        );
    }

    #[test]
    fn slash_in_an_offline_server_tab_is_refused() {
        let mut offline_home = online_home();
        offline_home.server_status = ServerStatus::Offline(RemoteError::Moved {
            server_name: home(),
        });
        let mut model = home_tab(offline_home, None);

        assert_eq!(press(&mut model, KeyCode::Char('/')), Err(Unhandled));
        assert_eq!(model.workspace.overlay, None);
    }

    #[test]
    fn each_edit_sends_a_search_with_a_new_revision() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(Vec::new(), None))),
        );

        let first = sent_revisions(&press(&mut model, KeyCode::Char('a')).unwrap());
        let second = sent_revisions(&press(&mut model, KeyCode::Char('b')).unwrap());

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].0, "a");
        assert_eq!(second[0].0, "ab");
        assert_ne!(first[0].1, second[0].1);
        assert_eq!(opened_query(&model).content.revision, Some(second[0].1));
    }

    #[test]
    fn emptying_the_input_orders_no_search_and_shows_no_rows() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(Vec::new(), None))),
        );
        let filled =
            sent_revisions(&press(&mut model, KeyCode::Char('a')).unwrap())[0].1;
        found(&mut model, Ok(vec![album_row("al-1")]), filled).unwrap();
        drop(press(&mut model, KeyCode::Char('b')).unwrap());
        let pending =
            sent_revisions(&press(&mut model, KeyCode::Backspace).unwrap())[0].1;

        let emptied = sent_revisions(&press(&mut model, KeyCode::Backspace).unwrap());

        assert_eq!(emptied, Vec::new());
        let server_query = opened_query(&model);
        assert_eq!(server_query.content.input, "");
        assert!(server_query.content.catalog_rows.is_empty());
        assert_eq!(server_query.content.revision, None);
        assert_eq!(server_query.cursor, Cursor::new(0));
        assert_eq!(
            found(&mut model, Ok(vec![album_row("late")]), pending),
            Err(Unhandled)
        );
        assert!(opened_query(&model).content.catalog_rows.is_empty());
    }

    #[test]
    fn found_fills_the_rows_only_for_the_current_revision() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(Vec::new(), None))),
        );
        let stale =
            sent_revisions(&press(&mut model, KeyCode::Char('a')).unwrap())[0].1;
        let current =
            sent_revisions(&press(&mut model, KeyCode::Char('b')).unwrap())[0].1;

        assert_eq!(
            found(&mut model, Ok(vec![album_row("old")]), stale),
            Err(Unhandled)
        );
        assert!(opened_query(&model).content.catalog_rows.is_empty());

        found(
            &mut model,
            Ok(vec![album_row("al-1"), track_row("tr-1")]),
            current,
        )
        .unwrap();

        let server_query = opened_query(&model);
        assert_eq!(
            server_query.content.catalog_rows,
            vec![album_row("al-1"), track_row("tr-1")]
        );
        assert_eq!(server_query.content.revision, None);
        assert_eq!(server_query.cursor, Cursor::new(2));
    }

    #[test]
    fn a_failed_search_is_dropped_unless_its_revision_is_current() {
        let error = RemoteError::Moved {
            server_name: home(),
        };
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(Vec::new(), None))),
        );
        let stale =
            sent_revisions(&press(&mut model, KeyCode::Char('a')).unwrap())[0].1;
        let current =
            sent_revisions(&press(&mut model, KeyCode::Char('b')).unwrap())[0].1;

        assert_eq!(found(&mut model, Err(error.clone()), stale), Err(Unhandled));
        assert_eq!(opened_query(&model).content.revision, Some(current));
        assert!(model.workspace.toasts.is_empty());

        found(&mut model, Err(error.clone()), current).unwrap();

        assert_eq!(opened_query(&model).content.revision, None);
        let [toast] = model.workspace.toasts.as_slice() else {
            panic!("expected one toast, got {:?}", model.workspace.toasts);
        };
        assert_eq!(toast.level, ToastLevel::Error);
        assert_eq!(toast.title, error.to_string());
    }

    #[test]
    fn enter_on_an_album_closes_the_search_and_opens_the_album_in_the_tab() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(
                vec![album_row("al-1"), track_row("tr-1")],
                None,
            ))),
        );

        let effects = press(&mut model, KeyCode::Enter).unwrap();

        assert_eq!(model.workspace.overlay, None);
        assert_eq!(
            model.catalogs[0]
                .album_level
                .as_ref()
                .map(|level| &level.listing),
            Some(&Listing::Album(AlbumId::new("al-1")))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Remote(RemoteCmd::List { .. })))
        );
    }

    #[test]
    fn enter_on_a_track_plays_it_with_the_found_tracks_as_the_playlist() {
        let mut query = server_query(
            vec![album_row("al-1"), track_row("tr-1"), track_row("tr-2")],
            None,
        );
        query.cursor = query.cursor.step(2);
        let mut model = home_tab(online_home(), Some(Overlay::ServerSearch(query)));

        press(&mut model, KeyCode::Enter).unwrap();

        assert_eq!(model.workspace.overlay, None);
        let second = TrackSource::Server {
            server_name: home(),
            server_track_id: ServerTrackId::new("tr-2"),
        };
        assert_eq!(model.playlist.tracks.len(), 2);
        assert_eq!(
            model.player.current().map(|track| track.source()),
            Some(&second)
        );
        assert_eq!(model.playlist_source, PlaylistSource::Server(home()));
    }

    #[test]
    fn esc_closes_the_server_search() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(Vec::new(), None))),
        );

        press(&mut model, KeyCode::Esc).unwrap();

        assert_eq!(model.workspace.overlay, None);
    }

    #[test]
    fn an_edit_after_the_server_goes_offline_is_refused_and_keeps_the_query() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(Vec::new(), None))),
        );
        drop(press(&mut model, KeyCode::Char('a')).unwrap());
        model.servers[0].server_status = ServerStatus::Offline(RemoteError::Moved {
            server_name: home(),
        });
        let before = opened_query(&model).clone();

        assert_eq!(press(&mut model, KeyCode::Char('b')), Err(Unhandled));
        assert_eq!(opened_query(&model), &before);
    }

    #[test]
    fn enqueue_in_the_server_search_is_refused() {
        let mut model = home_tab(
            online_home(),
            Some(Overlay::ServerSearch(server_query(
                vec![track_row("tr-1")],
                None,
            ))),
        );
        let before = model.workspace.overlay.clone();

        assert_eq!(press(&mut model, KeyCode::Tab), Err(Unhandled));
        assert_eq!(model.workspace.overlay, before);
    }
}
