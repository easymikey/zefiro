use std::{path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd, ScanMode},
    domain::{
        Browse,
        Cursor,
        Direction,
        Favorites,
        Moment,
        Player,
        ScanStatus,
        Track,
        TrackIndex,
        TrackRef,
        ViewIndex,
        Workspace,
        cycled,
        library::Library,
        playlist::{PlayOrder, Playlist, index_of_path},
    },
    message::{BrowseRequest, QueueRequest},
    update::{
        machine::{Machine, Unhandled},
        player::PlaybackParts,
    },
};

#[derive(Debug, Clone, Copy)]
pub enum BrowseMessage {
    CursorBy(isize),
    Top,
    Bottom,
    CursorTo(ViewIndex),
    PageBy(usize, Direction),
}

impl Machine for Browse {
    type Message = BrowseMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: BrowseMessage) -> Result<Cmd, Unhandled> {
        self.cursor = match message {
            BrowseMessage::CursorBy(delta) => self.cursor.step(delta),
            BrowseMessage::Top => self.cursor.first(),
            BrowseMessage::Bottom => self.cursor.last(),
            BrowseMessage::CursorTo(index) => {
                Cursor::with_len(self.cursor.len()).at(index.get())
            }
            BrowseMessage::PageBy(rows, direction) => self.cursor.page(rows, direction),
        };
        Ok(Cmd::none())
    }
}

fn navigate(
    workspace: &mut Workspace,
    message: BrowseMessage,
) -> Result<Cmd, Unhandled> {
    workspace.browse.transition(message)
}

pub(crate) struct BrowseParts<'a> {
    pub(crate) playback: PlaybackParts<'a>,
    pub(crate) library: &'a mut Option<Library>,
    pub(crate) favorites: &'a mut Favorites,
    pub(crate) scan_status: &'a mut ScanStatus,
    pub(crate) music_dir: &'a Path,
}

pub(crate) fn update(
    mut parts: BrowseParts<'_>,
    message: BrowseRequest,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let len = parts.playback.playlist.tracks.len();
    let workspace = &mut *parts.playback.workspace;
    workspace.browse.cursor = workspace.browse.cursor.resize(len);
    match message {
        BrowseRequest::ChordPrefix(prefix) => {
            workspace.chord_prefix = Some(prefix);
            Ok(Cmd::none())
        }
        BrowseRequest::CursorBy { rows } if len > 0 => {
            navigate(workspace, BrowseMessage::CursorBy(rows))
        }
        BrowseRequest::Top => navigate(workspace, BrowseMessage::Top),
        BrowseRequest::Bottom => navigate(workspace, BrowseMessage::Bottom),
        BrowseRequest::CycleSort => Ok(cycle_sort(&mut parts)),
        BrowseRequest::ToggleFavorite => Ok(toggle_favorite(&mut parts)),
        BrowseRequest::CursorTo(index) if len > 0 => {
            navigate(workspace, BrowseMessage::CursorTo(index))
        }
        BrowseRequest::PlaySelected => {
            let selected = workspace.browse.selected();
            crate::update::audio::jump_to(&mut parts.playback, selected, now)
        }
        BrowseRequest::PageBy(direction) if len > 0 => {
            let rows = workspace.visible_rows;
            navigate(workspace, BrowseMessage::PageBy(rows, direction))
        }
        BrowseRequest::FullScan => Ok(full_scan(&mut parts)),
        BrowseRequest::Trash(track_index) => Ok(trash_track(&mut parts, track_index)),
        BrowseRequest::SavePlaylist(name) => {
            Ok(Effect::Library(LibraryCmd::SavePlaylist {
                name,
                tracks: parts.playback.playlist.tracks.clone(),
            })
            .into())
        }
        BrowseRequest::CursorBy { .. }
        | BrowseRequest::CursorTo(_)
        | BrowseRequest::PageBy(_) => Ok(Cmd::none()),
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
    browse.cursor = browse.cursor.resize(playlist.tracks.len());
    let selected = source_at(playlist, browse.selected());
    Ok(match (message, selected) {
        (QueueRequest::EnqueueTrack(index), _) => source_at(playlist, index)
            .map_or_else(Cmd::none, |source| toggle_queued(queue, source)),
        (_, None) => Cmd::none(),
        (QueueRequest::Enqueue, Some(selected)) => toggle_queued(queue, selected),
        (QueueRequest::PlayNext, Some(selected)) => play_next(queue, selected),
        (QueueRequest::Dequeue, Some(selected)) => dequeue(queue, &selected),
        (QueueRequest::MoveInQueue(direction), Some(selected)) => {
            move_in_queue(queue, &selected, direction)
        }
    })
}

fn source_at(playlist: &Playlist, index: ViewIndex) -> Option<TrackRef> {
    playlist
        .tracks
        .get(index.get())
        .map(|track| track.source().clone())
}

fn full_scan(parts: &mut BrowseParts<'_>) -> Cmd {
    match parts.scan_status {
        ScanStatus::Idle => {
            *parts.scan_status = ScanStatus::Scanning;
            Effect::Library(LibraryCmd::Scan {
                music_dir: parts.music_dir.to_path_buf(),
                revision: parts.playback.revisions.issue_scan(),
                mode: ScanMode::Full,
            })
            .into()
        }
        ScanStatus::Scanning | ScanStatus::Tagging { .. } => Cmd::none(),
    }
}

fn toggle_favorite(parts: &mut BrowseParts<'_>) -> Cmd {
    let selected = parts.playback.workspace.browse.selected();
    let Some(track) = parts.playback.playlist.tracks.get(selected.get()) else {
        return Cmd::none();
    };
    parts.favorites.toggle(track.source().clone());
    Cmd::from_iter([
        Effect::Library(LibraryCmd::SaveFavorites(parts.favorites.clone())),
        Effect::Animate(Cue::FavoriteToggled),
    ])
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

fn play_next(queue: &mut Vec<TrackRef>, selected: TrackRef) -> Cmd {
    queue.retain(|queued| *queued != selected);
    queue.insert(0, selected);
    Cmd::none()
}

fn dequeue(queue: &mut Vec<TrackRef>, selected: &TrackRef) -> Cmd {
    queue.retain(|queued| queued != selected);
    Cmd::none()
}

fn move_in_queue(
    queue: &mut [TrackRef],
    selected: &TrackRef,
    direction: Direction,
) -> Cmd {
    let Some(index) = queue.iter().position(|queued| queued == selected) else {
        return Cmd::none();
    };
    let neighbor = match direction {
        Direction::Previous => index.checked_sub(1),
        Direction::Next => index.checked_add(1).filter(|&next| next < queue.len()),
    };
    if let Some(neighbor) = neighbor {
        queue.swap(index, neighbor);
    }
    Cmd::none()
}

fn cycle_sort(parts: &mut BrowseParts<'_>) -> Cmd {
    let browse = &mut parts.playback.workspace.browse;
    browse.sort = cycled(browse.sort, Direction::Next);
    let current: Vec<&Track> = parts
        .library
        .iter()
        .flat_map(Library::view_tracks)
        .map(|(_, track)| track.as_ref())
        .collect();
    let order =
        crate::domain::library::sort_indices(&current, browse.sort, parts.favorites);
    let Some(library) = parts.library.as_mut() else {
        return Cmd::none();
    };
    library.view = order
        .into_iter()
        .filter_map(|position| library.view.get(position.get()).copied())
        .collect();
    resync_playlist(ResyncParts {
        library,
        player: parts.playback.player,
        playlist: parts.playback.playlist,
    });
    Cmd::none()
}

pub(crate) struct ResyncParts<'a> {
    pub(crate) library: &'a mut Library,
    pub(crate) player: &'a Player,
    pub(crate) playlist: &'a mut Playlist,
}

fn trash_track(parts: &mut BrowseParts<'_>, track_index: ViewIndex) -> Cmd {
    let playback = &mut parts.playback;
    let Some(track) = playback.playlist.tracks.get(track_index.get()).cloned() else {
        return Cmd::none();
    };
    let Some(library) = parts.library.as_mut() else {
        return Cmd::none();
    };
    let removed_position = library.tracks.iter().position(|t| t.path() == track.path());
    library.tracks.retain(|t| t.path() != track.path());
    if let Some(removed_position) = removed_position {
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
    }
    playback.queue.retain(|queued| queued != track.source());
    resync_playlist(ResyncParts {
        library,
        player: playback.player,
        playlist: playback.playlist,
    });
    Cmd::from_iter([
        Effect::Library(LibraryCmd::Trash(track.path().to_path_buf())),
        Effect::Animate(Cue::TrackDeleted),
    ])
}

pub(crate) fn resync_playlist(parts: ResyncParts<'_>) {
    let ResyncParts {
        library,
        player,
        playlist,
    } = parts;
    let tracks: Vec<_> = library
        .view_tracks()
        .map(|(_, track)| Arc::clone(track))
        .collect();
    let anchor = player
        .current()
        .and_then(|track| index_of_path(track.path(), &tracks));
    playlist.relist(tracks, anchor);
    playlist.play_order =
        std::mem::replace(&mut playlist.play_order, PlayOrder::Linear).without_order();
}
