use std::{mem, path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, DiskCmd, Effect, LibraryCmd, RemoteCmd, ScanMode},
    domain::{
        catalog::{BrowseLevel, Catalog, CatalogName, Paging},
        cue::Cue,
        cursor::Cursor,
        cursor_over::cycled,
        direction::Direction,
        favorites::Favorites,
        geometry::Cells,
        history::HistoryEntry,
        index::ViewIndex,
        library::{Library, sort_indices},
        model::ScanStatus,
        overlay::Overlay,
        player::Player,
        playlist::{Playlist, PlaylistRows, PlaylistSource, index_of},
        server::{Listing, Page, Server, ServerStatus},
        time::Moment,
        toast::Toast,
        track::{CatalogRow, Track, TrackSource},
        workspace::{Browse, Workspace},
    },
    message::{BrowseRequest, Message, QueueRequest},
    update::{
        audio,
        machine::{Unhandled, replace},
        overlay::search,
        player::{self, events::PlaybackParts},
        server,
    },
};

pub(crate) struct BrowseParts<'a> {
    pub(crate) playback_parts: PlaybackParts<'a>,
    pub(crate) library: &'a mut Option<Library>,
    pub(crate) favorites: &'a mut Favorites,
    pub(crate) scan_status: &'a mut ScanStatus,
    pub(crate) music_dir: &'a Path,
    pub(crate) playlist_source: &'a mut PlaylistSource,
    pub(crate) catalog_name: &'a mut CatalogName,
    pub(crate) catalogs: &'a mut [Catalog],
}

pub(crate) fn update(
    mut parts: BrowseParts<'_>,
    request: BrowseRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let visible_rows = parts.playback_parts.workspace.visible_rows;
    let server_name = match &*parts.catalog_name {
        CatalogName::Local => return playlist(parts, request, now),
        CatalogName::Server(server_name) => server_name,
    };
    let server = parts
        .playback_parts
        .servers
        .iter()
        .find(|server| server.account.server_name == *server_name)
        .ok_or(Unhandled)?;
    let catalog = parts
        .catalogs
        .iter_mut()
        .find(|catalog| catalog.server_name == *server_name)
        .ok_or(Unhandled)?;
    match request {
        BrowseRequest::CursorBy { .. }
        | BrowseRequest::SelectFirst
        | BrowseRequest::SelectLast
        | BrowseRequest::PageBy(_) => {
            let level = catalog.level();
            let cursor =
                moved(level.cursor, &request, visible_rows).ok_or(Unhandled)?;
            replace(&mut level.cursor, cursor)?;
            server::catalog::moved(catalog);
        }
        BrowseRequest::PlaySelected => {
            let source = catalog.playlist_source();
            let level = catalog.level();
            let Some(selection) = album(level.rows(), level.cursor, source) else {
                let level = album_level(level.cursor.get(level.rows()), server)?;
                catalog.album_level = Some(level);
                return Ok(list(&mut parts));
            };
            return play_selected(&mut parts, selection, now);
        }
        BrowseRequest::CycleSort => cycle_sort(catalog, server)?,
        BrowseRequest::CycleView => cycle_view(catalog, server)?,
        BrowseRequest::LevelUp => {
            catalog.album_level.take().ok_or(Unhandled)?;
            server::catalog::show(catalog);
        }
        BrowseRequest::ToggleFavorite => return star(catalog, server, parts.favorites),
        BrowseRequest::StepCatalog(_) => return playlist(parts, request, now),
        BrowseRequest::Rescan
        | BrowseRequest::SavePlaylist(_)
        | BrowseRequest::Trash(_)
        | BrowseRequest::JumpTo(_) => return Err(Unhandled),
    }
    Ok(list(&mut parts))
}

fn list(parts: &mut BrowseParts<'_>) -> Cmd {
    server::catalog::list(
        parts.catalogs,
        parts.playback_parts.servers,
        parts.playback_parts.revisions,
    )
}

fn album_level(
    catalog_row: Option<&CatalogRow>,
    server: &Server,
) -> Result<BrowseLevel, Unhandled> {
    server::online(&server.server_status)?;
    match catalog_row.ok_or(Unhandled)? {
        CatalogRow::Album(server_album) => Ok(BrowseLevel {
            paging: Paging::Queued(Page::default()),
            ..BrowseLevel::new(Listing::Album(server_album.album_id.clone()))
        }),
        CatalogRow::Playlist(server_playlist) => Ok(BrowseLevel {
            paging: Paging::Queued(Page::default()),
            ..BrowseLevel::new(Listing::Playlist(server_playlist.playlist_id.clone()))
        }),
        CatalogRow::Track(_) => Err(Unhandled),
    }
}

fn star(
    catalog: &mut Catalog,
    server: &Server,
    favorites: &mut Favorites,
) -> Result<Cmd, Unhandled> {
    let session = match &server.server_status {
        ServerStatus::Online(session) => session,
        ServerStatus::Connecting | ServerStatus::Offline(_) => return Err(Unhandled),
    };
    let level = catalog.level();
    let track_source = match level.cursor.get(level.rows()).ok_or(Unhandled)? {
        CatalogRow::Track(track) => track.source(),
        CatalogRow::Album(_) | CatalogRow::Playlist(_) => {
            return Err(Unhandled);
        }
    };
    let favorite = !favorites.favorite(track_source);
    let remote_cmd = match track_source {
        TrackSource::Server {
            server_name,
            server_track_id,
        } => RemoteCmd::Star {
            server_name: server_name.clone(),
            session: session.clone(),
            server_track_id: server_track_id.clone(),
            favorite,
        },
        TrackSource::Local(_path) => return Err(Unhandled),
    };
    favorites.set(track_source.clone(), favorite);
    Ok(Cmd::from_iter([
        Effect::Remote(remote_cmd),
        Effect::Animate(Cue::FavoriteToggled),
    ]))
}

fn album(
    catalog_rows: &[CatalogRow],
    cursor: Cursor,
    playlist_source: PlaylistSource,
) -> Option<Selection> {
    let Some(CatalogRow::Track(selected)) = cursor.get(catalog_rows) else {
        return None;
    };
    let tracks: Vec<Arc<Track>> = catalog_rows
        .iter()
        .filter_map(|catalog_row| match catalog_row {
            CatalogRow::Track(track) => Some(Arc::clone(track)),
            CatalogRow::Album(_) | CatalogRow::Playlist(_) => None,
        })
        .collect();
    let index = index_of(&tracks, selected.source())?;
    let mut playlist = Playlist::from_tracks(tracks);
    playlist.jump(ViewIndex::new(index))?;
    Some(Selection {
        playlist,
        playlist_source,
    })
}

struct Selection {
    playlist: Playlist,
    playlist_source: PlaylistSource,
}

fn play_selected(
    parts: &mut BrowseParts<'_>,
    selection: Selection,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let Selection {
        playlist,
        playlist_source,
    } = selection;
    let track = playlist.current().cloned().ok_or(Unhandled)?;
    let cmd = match player::start(&mut parts.playback_parts, track, now) {
        Ok(cmd) => cmd,
        Err(offline_toast) => return Ok(offline_toast),
    };
    let anchor = playlist.playing_index();
    parts
        .playback_parts
        .playlist
        .relist(playlist.tracks, anchor);
    *parts.playlist_source = playlist_source;
    if let Some(library) = parts.library.as_ref() {
        resync_playlist(
            parts.playlist_source,
            library,
            ResyncParts {
                workspace: &mut *parts.playback_parts.workspace,
                player: parts.playback_parts.player,
                playlist: parts.playback_parts.playlist,
            },
        );
    } else {
        let rows = PlaylistRows::new(
            parts.playlist_source,
            None,
            parts.playback_parts.playlist,
        );
        let browse = &mut parts.playback_parts.workspace.browse;
        browse.cursor = browse.cursor.resize(rows.len());
    }
    Ok(cmd)
}

fn moved(
    cursor: Cursor,
    request: &BrowseRequest,
    visible_rows: Cells,
) -> Option<Cursor> {
    match request {
        BrowseRequest::CursorBy { rows } => {
            (!cursor.is_empty()).then(|| cursor.step(*rows))
        }
        BrowseRequest::SelectFirst => Some(cursor.first()),
        BrowseRequest::SelectLast => Some(cursor.last()),
        BrowseRequest::PageBy(direction) => (!cursor.is_empty()
            && visible_rows != Cells(0))
        .then(|| cursor.page(visible_rows.count(), *direction)),
        BrowseRequest::Trash(_)
        | BrowseRequest::SavePlaylist(_)
        | BrowseRequest::PlaySelected
        | BrowseRequest::JumpTo(_)
        | BrowseRequest::CycleSort
        | BrowseRequest::CycleView
        | BrowseRequest::Rescan
        | BrowseRequest::ToggleFavorite
        | BrowseRequest::StepCatalog(_)
        | BrowseRequest::LevelUp => None,
    }
}

fn catalog(
    parts: &mut BrowseParts<'_>,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let index = match &*parts.catalog_name {
        CatalogName::Local => 0,
        CatalogName::Server(server_name) => parts
            .playback_parts
            .servers
            .iter()
            .position(|server| server.account.server_name == *server_name)
            .map_or(0, |position| position + 1),
    };
    let next = direction.wrapped(index, parts.playback_parts.servers.len() + 1);
    if next == index {
        return Err(Unhandled);
    }
    let Some(server) = next
        .checked_sub(1)
        .and_then(|position| parts.playback_parts.servers.get(position))
    else {
        *parts.catalog_name = CatalogName::Local;
        return Ok(Cmd::none());
    };
    let catalog = parts
        .catalogs
        .iter_mut()
        .find(|catalog| catalog.server_name == server.account.server_name)
        .ok_or(Unhandled)?;
    *parts.catalog_name = CatalogName::Server(catalog.server_name.clone());
    server::catalog::show(catalog);
    Ok(list(parts))
}

fn playlist(
    mut parts: BrowseParts<'_>,
    request: BrowseRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let workspace = &mut *parts.playback_parts.workspace;
    match request {
        BrowseRequest::CursorBy { .. }
        | BrowseRequest::SelectFirst
        | BrowseRequest::SelectLast
        | BrowseRequest::PageBy(_) => {
            let cursor =
                moved(workspace.browse.cursor, &request, workspace.visible_rows)
                    .ok_or(Unhandled)?;
            replace(&mut workspace.browse.cursor, cursor).map(|()| Cmd::none())
        }
        BrowseRequest::CycleSort => {
            workspace.browse.sort_key =
                cycled(workspace.browse.sort_key, Direction::Next);
            let sort_key = workspace.browse.sort_key;
            let Some(library) = parts.library.as_mut() else {
                return Ok(Cmd::none());
            };
            library.track_indexes =
                sort_indices(&library.tracks, sort_key, parts.favorites);
            resync_playlist(
                parts.playlist_source,
                library,
                ResyncParts {
                    workspace: &mut *parts.playback_parts.workspace,
                    player: parts.playback_parts.player,
                    playlist: parts.playback_parts.playlist,
                },
            );
            Ok(Cmd::none())
        }
        BrowseRequest::ToggleFavorite => toggle_favorite(&mut parts),
        BrowseRequest::PlaySelected => {
            let selected = workspace.browse.selected();
            jump_to(&mut parts, selected, now)
        }
        BrowseRequest::JumpTo(index) => jump_to(&mut parts, index, now),
        BrowseRequest::Rescan => full_scan(&mut parts),
        BrowseRequest::SavePlaylist(name) => {
            Ok(Effect::Library(LibraryCmd::Disk(DiskCmd::SavePlaylist {
                name,
                tracks: PlaylistRows::new(
                    parts.playlist_source,
                    parts.library.as_ref(),
                    parts.playback_parts.playlist,
                )
                .iter()
                .map(Arc::clone)
                .collect(),
            }))
            .into())
        }
        BrowseRequest::Trash(source) => trash_track(&parts, &source),
        BrowseRequest::StepCatalog(direction) => catalog(&mut parts, direction),
        BrowseRequest::CycleView | BrowseRequest::LevelUp => Err(Unhandled),
    }
}

fn jump_to(
    parts: &mut BrowseParts<'_>,
    index: ViewIndex,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if parts.playlist_source.server_name().is_some() {
        let library = parts.library.as_ref().ok_or(Unhandled)?;
        library.view_track(index).ok_or(Unhandled)?;
        *parts.playlist_source = PlaylistSource::Library;
        resync_playlist(
            parts.playlist_source,
            library,
            ResyncParts {
                workspace: &mut *parts.playback_parts.workspace,
                player: parts.playback_parts.player,
                playlist: parts.playback_parts.playlist,
            },
        );
    }
    audio::jump_to(&mut parts.playback_parts, index, now)
}

pub(crate) struct QueueParts<'a> {
    pub(crate) catalog_name: &'a CatalogName,
    pub(crate) rows: PlaylistRows<'a>,
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) browse: &'a mut Browse,
    pub(crate) queue: &'a mut Vec<TrackSource>,
}

pub(crate) fn queue(
    parts: QueueParts<'_>,
    request: QueueRequest,
) -> Result<Cmd, Unhandled> {
    let QueueParts {
        catalog_name,
        rows,
        history,
        browse,
        queue,
    } = parts;
    let at_cursor =
        || source_at(rows, ViewIndex::new(browse.cursor.index())).ok_or(Unhandled);
    match request {
        QueueRequest::Toggle
        | QueueRequest::PlayNext
        | QueueRequest::Dequeue
        | QueueRequest::Move(_)
            if matches!(catalog_name, CatalogName::Server(_)) =>
        {
            Err(Unhandled)
        }
        QueueRequest::ToggleAt(index) => source_at(rows, index)
            .map(|source| toggle_queued(queue, source))
            .ok_or(Unhandled),
        QueueRequest::ToggleHistoryEntry(position) => {
            let history_entry = history.get(position).ok_or(Unhandled)?;
            Ok(enqueue_history_entry(queue, rows, history_entry))
        }
        QueueRequest::Toggle => Ok(toggle_queued(queue, at_cursor()?)),
        QueueRequest::PlayNext => play_next(queue, at_cursor()?),
        QueueRequest::Dequeue => dequeue(queue, &at_cursor()?),
        QueueRequest::Move(direction) => move_in_queue(queue, &at_cursor()?, direction),
    }
}

fn enqueue_history_entry(
    queue: &mut Vec<TrackSource>,
    rows: PlaylistRows<'_>,
    history_entry: &HistoryEntry,
) -> Cmd {
    if rows
        .iter()
        .any(|track| *track.source() == history_entry.track_source)
    {
        toggle_queued(queue, history_entry.track_source.clone())
    } else {
        Cmd::message(Message::Toast(Toast::info("Not in library".to_string())))
    }
}

fn source_at(rows: PlaylistRows<'_>, index: ViewIndex) -> Option<TrackSource> {
    rows.get(index)
        .filter(|track| track.local_path().is_some())
        .map(|track| track.source().clone())
}

fn full_scan(parts: &mut BrowseParts<'_>) -> Result<Cmd, Unhandled> {
    match parts.scan_status {
        ScanStatus::Idle => {
            *parts.scan_status = ScanStatus::Scanning;
            Ok(Effect::Library(LibraryCmd::Scan {
                music_dir: parts.music_dir.to_path_buf(),
                revision: parts.playback_parts.revisions.issue_scan(),
                mode: ScanMode::Fresh,
            })
            .into())
        }
        ScanStatus::Scanning | ScanStatus::Tagging { .. } => Err(Unhandled),
    }
}

fn toggle_favorite(parts: &mut BrowseParts<'_>) -> Result<Cmd, Unhandled> {
    let selected = parts.playback_parts.workspace.browse.selected();
    let track = PlaylistRows::new(
        parts.playlist_source,
        parts.library.as_ref(),
        parts.playback_parts.playlist,
    )
    .get(selected)
    .ok_or(Unhandled)?;
    parts.favorites.toggle(track.source().clone());
    Ok(Cmd::from_iter([
        Effect::Library(LibraryCmd::Disk(DiskCmd::SaveFavorites(
            parts.favorites.clone(),
        ))),
        Effect::Animate(Cue::FavoriteToggled),
    ]))
}

fn toggle_queued(queue: &mut Vec<TrackSource>, source: TrackSource) -> Cmd {
    match queue.iter().position(|queued| *queued == source) {
        Some(position) => {
            queue.remove(position);
        }
        None => queue.push(source),
    }
    Cue::QueueChanged.into()
}

fn play_next(
    queue: &mut Vec<TrackSource>,
    track_source: TrackSource,
) -> Result<Cmd, Unhandled> {
    if queue.first() == Some(&track_source) {
        return Err(Unhandled);
    }
    queue.retain(|queued| *queued != track_source);
    queue.insert(0, track_source);
    Ok(Cue::QueueChanged.into())
}

fn dequeue(
    queue: &mut Vec<TrackSource>,
    track_source: &TrackSource,
) -> Result<Cmd, Unhandled> {
    if !queue.contains(track_source) {
        return Err(Unhandled);
    }
    queue.retain(|queued| queued != track_source);
    Ok(Cue::QueueChanged.into())
}

fn move_in_queue(
    queue: &mut [TrackSource],
    track_source: &TrackSource,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let index = queue
        .iter()
        .position(|queued| queued == track_source)
        .ok_or(Unhandled)?;
    let neighbor = match direction {
        Direction::Previous => index.checked_sub(1),
        Direction::Next => index.checked_add(1).filter(|&next| next < queue.len()),
    }
    .ok_or(Unhandled)?;
    queue.swap(index, neighbor);
    Ok(Cue::QueueChanged.into())
}

fn cycle_sort(catalog: &mut Catalog, server: &Server) -> Result<(), Unhandled> {
    server::online(&server.server_status)?;
    let album_order = match (&catalog.album_level, &catalog.albums_level.listing) {
        (None, Listing::Albums(album_order)) => cycled(*album_order, Direction::Next),
        (
            Some(_),
            Listing::Songs
            | Listing::Albums(_)
            | Listing::Album(_)
            | Listing::Playlists
            | Listing::Playlist(_),
        )
        | (
            None,
            Listing::Songs
            | Listing::Album(_)
            | Listing::Playlists
            | Listing::Playlist(_),
        ) => {
            return Err(Unhandled);
        }
    };
    catalog.albums_level.listing = Listing::Albums(album_order);
    catalog.albums_level.paging = Paging::Queued(Page::default());
    Ok(())
}

fn cycle_view(catalog: &mut Catalog, server: &Server) -> Result<(), Unhandled> {
    server::online(&server.server_status)?;
    match catalog.albums_level.listing {
        Listing::Songs | Listing::Albums(_) | Listing::Playlists => {}
        Listing::Album(_) | Listing::Playlist(_) => return Err(Unhandled),
    }
    catalog.album_level = None;
    let [next, after] = &mut catalog.browse_levels;
    mem::swap(&mut catalog.albums_level, next);
    mem::swap(next, after);
    server::catalog::show(catalog);
    Ok(())
}

pub(crate) struct ResyncParts<'a> {
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) player: &'a Player,
    pub(crate) playlist: &'a mut Playlist,
}

fn trash_track(
    parts: &BrowseParts<'_>,
    source: &TrackSource,
) -> Result<Cmd, Unhandled> {
    let library = parts.library.as_ref().ok_or(Unhandled)?;
    let path = library
        .tracks
        .iter()
        .find(|track| track.source() == source)
        .ok_or(Unhandled)?
        .local_path()
        .ok_or(Unhandled)?
        .to_path_buf();
    Ok(Effect::Library(LibraryCmd::Disk(DiskCmd::Trash(path))).into())
}

pub(crate) fn resync_playlist(
    source: &PlaylistSource,
    library: &Library,
    parts: ResyncParts<'_>,
) {
    match source {
        PlaylistSource::Library => {
            let tracks = library
                .view_tracks()
                .map(|(_, track)| Arc::clone(track))
                .collect();
            relist(tracks, parts);
        }
        PlaylistSource::Named => {}
        PlaylistSource::Server(_) | PlaylistSource::Songs(_) => {
            let browse = &mut parts.workspace.browse;
            browse.cursor = browse.cursor.resize(library.track_indexes.len());
            if let Some(Overlay::Search(search)) = parts.workspace.overlay.as_mut() {
                search::rerank(search, PlaylistRows::Library(library));
            }
        }
    }
}

pub(crate) fn relist(
    tracks: Vec<Arc<Track>>,
    ResyncParts {
        workspace,
        player,
        playlist,
    }: ResyncParts<'_>,
) {
    let anchor = player
        .current()
        .and_then(|track| index_of(&tracks, track.source()))
        .map(ViewIndex::new);
    playlist.relist(tracks, anchor);
    workspace.browse.cursor = workspace.browse.cursor.resize(playlist.tracks.len());
    if let Some(Overlay::Search(search)) = workspace.overlay.as_mut() {
        search::rerank(search, PlaylistRows::Tracks(&playlist.tracks));
    }
}
