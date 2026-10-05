use std::{path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, Effect, LibraryCmd, ScanMode},
    domain::{
        cue::Cue,
        cursor_over::cycled,
        direction::Direction,
        favorites::Favorites,
        geometry::Cells,
        index::{TrackIndex, ViewIndex},
        library::{Library, sort_indices},
        model::ScanStatus,
        player::Player,
        playlist::{PlayOrder, Playlist, PlaylistSource, index_of},
        time::Moment,
        track::TrackRef,
        workspace::Browse,
    },
    message::{BrowseRequest, QueueRequest},
    update::{machine::Unhandled, player::PlaybackParts},
};

pub(crate) struct BrowseParts<'a> {
    pub(crate) playback: PlaybackParts<'a>,
    pub(crate) library: &'a mut Option<Library>,
    pub(crate) favorites: &'a mut Favorites,
    pub(crate) scan_status: &'a mut ScanStatus,
    pub(crate) music_dir: &'a Path,
    pub(crate) playlist_source: &'a PlaylistSource,
}

pub(crate) fn update(
    mut parts: BrowseParts<'_>,
    message: BrowseRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if let BrowseRequest::Trash(source) = &message {
        return trash_track(&mut parts, source);
    }
    let len = parts.playback.playlist.tracks.len();
    let workspace = &mut *parts.playback.workspace;
    if refused(&message, len, workspace.visible_rows) {
        return Err(Unhandled);
    }
    match message {
        BrowseRequest::ChordPrefix(prefix) => {
            workspace.chord_prefix = Some(prefix);
            Ok(Cmd::none())
        }
        BrowseRequest::CursorBy { rows } => {
            workspace.browse.cursor = workspace.browse.cursor.step(rows);
            Ok(Cmd::none())
        }
        BrowseRequest::Top => {
            workspace.browse.cursor = workspace.browse.cursor.first();
            Ok(Cmd::none())
        }
        BrowseRequest::Bottom => {
            workspace.browse.cursor = workspace.browse.cursor.last();
            Ok(Cmd::none())
        }
        BrowseRequest::CycleSort => Ok(cycle_sort(&mut parts)),
        BrowseRequest::ToggleFavorite => toggle_favorite(&mut parts),
        BrowseRequest::PlaySelected => {
            let selected = workspace.browse.selected();
            crate::update::audio::jump_to(&mut parts.playback, selected, now)
        }
        BrowseRequest::PageBy(direction) => {
            let rows = workspace.visible_rows.count();
            workspace.browse.cursor = workspace.browse.cursor.page(rows, direction);
            Ok(Cmd::none())
        }
        BrowseRequest::FullScan => full_scan(&mut parts),
        BrowseRequest::SavePlaylist(name) => {
            Ok(Effect::Library(LibraryCmd::SavePlaylist {
                name,
                tracks: parts.playback.playlist.tracks.clone(),
            })
            .into())
        }
        BrowseRequest::Trash(_) => Err(Unhandled),
    }
}

fn refused(message: &BrowseRequest, len: usize, visible_rows: Cells) -> bool {
    match message {
        BrowseRequest::CursorBy { .. } | BrowseRequest::ToggleFavorite => len == 0,
        BrowseRequest::PageBy(_) => len == 0 || visible_rows == Cells(0),
        BrowseRequest::ChordPrefix(_)
        | BrowseRequest::Top
        | BrowseRequest::Bottom
        | BrowseRequest::CycleSort
        | BrowseRequest::PlaySelected
        | BrowseRequest::FullScan
        | BrowseRequest::SavePlaylist(_)
        | BrowseRequest::Trash(_) => false,
    }
}

pub(crate) struct QueueParts<'a> {
    pub(crate) playlist: &'a Playlist,
    pub(crate) browse: &'a mut Browse,
    pub(crate) queue: &'a mut Vec<TrackRef>,
}

pub(crate) fn queue(
    parts: QueueParts<'_>,
    message: QueueRequest,
) -> Result<Cmd, Unhandled> {
    let QueueParts {
        playlist,
        browse,
        queue,
    } = parts;
    let selected = source_at(playlist, ViewIndex::new(browse.cursor.index()));
    match (message, selected) {
        (QueueRequest::EnqueueTrack(index), _) => source_at(playlist, index)
            .map(|source| toggle_queued(queue, source))
            .ok_or(Unhandled),
        (_, None) => Err(Unhandled),
        (QueueRequest::Enqueue, Some(selected)) => Ok(toggle_queued(queue, selected)),
        (QueueRequest::PlayNext, Some(selected)) => play_next(queue, selected),
        (QueueRequest::Dequeue, Some(selected)) => dequeue(queue, &selected),
        (QueueRequest::MoveInQueue(direction), Some(selected)) => {
            move_in_queue(queue, &selected, direction)
        }
    }
}

fn source_at(playlist: &Playlist, index: ViewIndex) -> Option<TrackRef> {
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
                revision: parts.playback.revisions.issue_scan(),
                mode: ScanMode::Full,
            })
            .into())
        }
        ScanStatus::Scanning | ScanStatus::Tagging { .. } => Err(Unhandled),
    }
}

fn toggle_favorite(parts: &mut BrowseParts<'_>) -> Result<Cmd, Unhandled> {
    let selected = parts.playback.workspace.browse.selected();
    let track = parts
        .playback
        .playlist
        .tracks
        .get(selected.get())
        .ok_or(Unhandled)?;
    parts.favorites.toggle(track.source().clone());
    Ok(Cmd::from_iter([
        Effect::Library(LibraryCmd::SaveFavorites(parts.favorites.clone())),
        Effect::Animate(Cue::FavoriteToggled),
    ]))
}

fn toggle_queued(queue: &mut Vec<TrackRef>, source: TrackRef) -> Cmd {
    match queue.iter().position(|queued| *queued == source) {
        Some(position) => {
            queue.remove(position);
        }
        None => queue.push(source),
    }
    Cue::QueueChanged.into()
}

fn play_next(queue: &mut Vec<TrackRef>, selected: TrackRef) -> Result<Cmd, Unhandled> {
    if queue.first() == Some(&selected) {
        return Err(Unhandled);
    }
    queue.retain(|queued| *queued != selected);
    queue.insert(0, selected);
    Ok(Cue::QueueChanged.into())
}

fn dequeue(queue: &mut Vec<TrackRef>, selected: &TrackRef) -> Result<Cmd, Unhandled> {
    let before = queue.len();
    queue.retain(|queued| queued != selected);
    if queue.len() == before {
        return Err(Unhandled);
    }
    Ok(Cue::QueueChanged.into())
}

fn move_in_queue(
    queue: &mut [TrackRef],
    selected: &TrackRef,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let index = queue
        .iter()
        .position(|queued| queued == selected)
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
    let browse = &mut parts.playback.workspace.browse;
    browse.sort = cycled(browse.sort, Direction::Next);
    let sort = browse.sort;
    let Some(library) = parts.library.as_mut() else {
        return Cmd::none();
    };
    library.view = sort_indices(&library.tracks, sort, parts.favorites);
    resync_playlist(
        *parts.playlist_source,
        ResyncParts {
            library,
            browse: &mut parts.playback.workspace.browse,
            player: parts.playback.player,
            playlist: parts.playback.playlist,
        },
    );
    Cmd::none()
}

pub(crate) struct ResyncParts<'a> {
    pub(crate) library: &'a mut Library,
    pub(crate) browse: &'a mut Browse,
    pub(crate) player: &'a Player,
    pub(crate) playlist: &'a mut Playlist,
}

fn trash_track(
    parts: &mut BrowseParts<'_>,
    source: &TrackRef,
) -> Result<Cmd, Unhandled> {
    let playback = &mut parts.playback;
    let library = parts.library.as_mut().ok_or(Unhandled)?;
    let removed_position = index_of(&library.tracks, source).ok_or(Unhandled)?;
    let track = library.tracks.remove(removed_position);
    library.view = library
        .view
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
            browse: &mut playback.workspace.browse,
            player: playback.player,
            playlist: playback.playlist,
        },
    );
    Ok(Cmd::from_iter([
        Effect::Library(LibraryCmd::Trash(track.path().to_path_buf())),
        Effect::Animate(Cue::TrackDeleted),
    ]))
}

pub(crate) fn resync_playlist(source: PlaylistSource, parts: ResyncParts<'_>) {
    if source == PlaylistSource::Named {
        return;
    }
    let ResyncParts {
        library,
        browse,
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
    browse.cursor = browse.cursor.resize(playlist.tracks.len());
}
