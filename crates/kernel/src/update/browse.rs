use std::{path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, DiskCmd, Effect, LibraryCmd, ScanMode},
    domain::{
        cue::Cue,
        cursor_over::cycled,
        direction::Direction,
        favorites::Favorites,
        geometry::Cells,
        history::HistoryEntry,
        index::{TrackIndex, ViewIndex},
        library::{Library, sort_indices},
        model::ScanStatus,
        overlay::Overlay,
        player::Player,
        playlist::{PlayOrder, Playlist, PlaylistSource, index_of},
        time::Moment,
        toast::Toast,
        track::TrackSource,
        workspace::{Browse, Workspace},
    },
    message::{BrowseRequest, Message, QueueRequest},
    update::{
        machine::{Unhandled, move_cursor},
        player::PlaybackParts,
    },
};

pub(crate) struct BrowseParts<'a> {
    pub(crate) playback_parts: PlaybackParts<'a>,
    pub(crate) library: &'a mut Option<Library>,
    pub(crate) favorites: &'a mut Favorites,
    pub(crate) scan_status: &'a mut ScanStatus,
    pub(crate) music_dir: &'a Path,
    pub(crate) playlist_source: &'a PlaylistSource,
}

pub(crate) fn update(
    mut parts: BrowseParts<'_>,
    request: BrowseRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if let BrowseRequest::Trash(source) = &request {
        return trash_track(&mut parts, source);
    }
    let len = parts.playback_parts.playlist.tracks.len();
    let workspace = &mut *parts.playback_parts.workspace;
    if is_refused(&request, len, workspace.visible_rows) {
        return Err(Unhandled);
    }
    match request {
        BrowseRequest::CursorBy { rows } => {
            let moved = workspace.browse.cursor.step(rows);
            move_cursor(&mut workspace.browse.cursor, moved)
        }
        BrowseRequest::SelectFirst => {
            let moved = workspace.browse.cursor.first();
            move_cursor(&mut workspace.browse.cursor, moved)
        }
        BrowseRequest::SelectLast => {
            let moved = workspace.browse.cursor.last();
            move_cursor(&mut workspace.browse.cursor, moved)
        }
        BrowseRequest::CycleSort => Ok(cycle_sort(&mut parts)),
        BrowseRequest::ToggleFavorite => toggle_favorite(&mut parts),
        BrowseRequest::PlaySelected => {
            let selected = workspace.browse.selected();
            crate::update::audio::jump_to(&mut parts.playback_parts, selected, now)
        }
        BrowseRequest::PageBy(direction) => {
            let rows = workspace.visible_rows.count();
            let moved = workspace.browse.cursor.page(rows, direction);
            move_cursor(&mut workspace.browse.cursor, moved)
        }
        BrowseRequest::Rescan => full_scan(&mut parts),
        BrowseRequest::SavePlaylist(name) => {
            Ok(Effect::Library(LibraryCmd::Disk(DiskCmd::SavePlaylist {
                name,
                tracks: parts.playback_parts.playlist.tracks.clone(),
            }))
            .into())
        }
        BrowseRequest::Trash(_) => Err(Unhandled),
    }
}

fn is_refused(request: &BrowseRequest, len: usize, visible_rows: Cells) -> bool {
    match request {
        BrowseRequest::CursorBy { .. } | BrowseRequest::ToggleFavorite => len == 0,
        BrowseRequest::PageBy(_) => len == 0 || visible_rows == Cells(0),
        BrowseRequest::SelectFirst
        | BrowseRequest::SelectLast
        | BrowseRequest::CycleSort
        | BrowseRequest::PlaySelected
        | BrowseRequest::Rescan
        | BrowseRequest::SavePlaylist(_)
        | BrowseRequest::Trash(_) => false,
    }
}

pub(crate) struct QueueParts<'a> {
    pub(crate) playlist: &'a Playlist,
    pub(crate) history: &'a [HistoryEntry],
    pub(crate) browse: &'a mut Browse,
    pub(crate) queue: &'a mut Vec<TrackSource>,
}

pub(crate) fn queue(
    parts: QueueParts<'_>,
    request: QueueRequest,
) -> Result<Cmd, Unhandled> {
    let QueueParts {
        playlist,
        history,
        browse,
        queue,
    } = parts;
    let track_source = source_at(playlist, ViewIndex::new(browse.cursor.index()));
    match (request, track_source) {
        (QueueRequest::ToggleAt(index), _) => source_at(playlist, index)
            .map(|source| toggle_queued(queue, source))
            .ok_or(Unhandled),
        (QueueRequest::ToggleHistoryEntry(position), _) => {
            let history_entry = history.get(position).ok_or(Unhandled)?;
            Ok(enqueue_history_entry(queue, playlist, history_entry))
        }
        (_, None) => Err(Unhandled),
        (QueueRequest::Toggle, Some(track_source)) => {
            Ok(toggle_queued(queue, track_source))
        }
        (QueueRequest::PlayNext, Some(track_source)) => play_next(queue, track_source),
        (QueueRequest::Dequeue, Some(track_source)) => dequeue(queue, &track_source),
        (QueueRequest::Move(direction), Some(track_source)) => {
            move_in_queue(queue, &track_source, direction)
        }
    }
}

fn enqueue_history_entry(
    queue: &mut Vec<TrackSource>,
    playlist: &Playlist,
    history_entry: &HistoryEntry,
) -> Cmd {
    index_of(&playlist.tracks, &history_entry.track_source)
        .and_then(|index| source_at(playlist, ViewIndex::new(index)))
        .map_or_else(
            || Cmd::message(Message::Toast(Toast::info("Not in library".to_string()))),
            |source| toggle_queued(queue, source),
        )
}

fn source_at(playlist: &Playlist, index: ViewIndex) -> Option<TrackSource> {
    playlist
        .tracks
        .get(index.get())
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
    let track = parts
        .playback_parts
        .playlist
        .tracks
        .get(selected.get())
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
    let before = queue.len();
    queue.retain(|queued| queued != track_source);
    if queue.len() == before {
        return Err(Unhandled);
    }
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

fn cycle_sort(parts: &mut BrowseParts<'_>) -> Cmd {
    let browse = &mut parts.playback_parts.workspace.browse;
    browse.sort_key = cycled(browse.sort_key, Direction::Next);
    let sort_key = browse.sort_key;
    let Some(library) = parts.library.as_mut() else {
        return Cmd::none();
    };
    library.track_indexes = sort_indices(&library.tracks, sort_key, parts.favorites);
    resync_playlist(
        *parts.playlist_source,
        ResyncParts {
            library,
            workspace: &mut *parts.playback_parts.workspace,
            player: parts.playback_parts.player,
            playlist: parts.playback_parts.playlist,
        },
    );
    Cmd::none()
}

pub(crate) struct ResyncParts<'a> {
    pub(crate) library: &'a mut Library,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) player: &'a Player,
    pub(crate) playlist: &'a mut Playlist,
}

fn trash_track(
    parts: &mut BrowseParts<'_>,
    source: &TrackSource,
) -> Result<Cmd, Unhandled> {
    let playback = &mut parts.playback_parts;
    let library = parts.library.as_mut().ok_or(Unhandled)?;
    let removed_position = index_of(&library.tracks, source).ok_or(Unhandled)?;
    let track = library.tracks.remove(removed_position);
    library.track_indexes = library
        .track_indexes
        .iter()
        .filter(|index| index.get() != removed_position)
        .map(|index| {
            if index.get() > removed_position {
                TrackIndex::new(index.get() - 1)
            } else {
                *index
            }
        })
        .collect();
    playback.queue.retain(|queued| queued != source);
    resync_playlist(
        *parts.playlist_source,
        ResyncParts {
            library,
            workspace: &mut *playback.workspace,
            player: playback.player,
            playlist: playback.playlist,
        },
    );
    Ok(Cmd::from_iter([
        Effect::Library(LibraryCmd::Disk(DiskCmd::Trash(track.path().to_path_buf()))),
        Effect::Animate(Cue::TrackTrashed),
    ]))
}

pub(crate) fn resync_playlist(source: PlaylistSource, parts: ResyncParts<'_>) {
    if source == PlaylistSource::Named {
        return;
    }
    let ResyncParts {
        library,
        workspace,
        player,
        playlist,
    } = parts;
    let tracks: Vec<_> = library
        .view_tracks()
        .map(|(_, track)| Arc::clone(track))
        .collect();
    let anchor = player
        .current()
        .and_then(|track| index_of(&tracks, track.source()))
        .map(ViewIndex::new);
    playlist.relist(tracks, anchor);
    playlist.play_order =
        std::mem::replace(&mut playlist.play_order, PlayOrder::Linear).without_order();
    workspace.browse.cursor = workspace.browse.cursor.resize(playlist.tracks.len());
    if let Some(Overlay::Search(search)) = workspace.overlay.as_mut() {
        crate::update::overlay::search::rerank(search, &playlist.tracks);
    }
}
