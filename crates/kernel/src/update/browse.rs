use std::sync::Arc;

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd, ScanMode},
    domain::{
        Browse,
        Cursor,
        Direction,
        Model,
        Moment,
        Player,
        ScanStatus,
        Track,
        TrackIndex,
        ViewIndex,
        Workspace,
        cycled,
        library::Library,
        playlist::{Playlist, index_of_path},
    },
    message::{BrowseRequest, QueueRequest},
    update::error::UpdateError,
};

#[derive(Debug, Clone, Copy)]
pub enum BrowseMessage {
    CursorBy(isize),
    Top,
    Bottom,
    CursorTo(ViewIndex),
    PageBy(usize, Direction),
}

impl Browse {
    pub fn apply(&mut self, message: BrowseMessage) -> Cmd {
        self.cursor = match message {
            BrowseMessage::CursorBy(delta) => self.cursor.step(delta),
            BrowseMessage::Top => self.cursor.first(),
            BrowseMessage::Bottom => self.cursor.last(),
            BrowseMessage::CursorTo(index) => {
                Cursor::with_len(self.cursor.len()).at(index.get())
            }
            BrowseMessage::PageBy(rows, direction) => self.cursor.page(rows, direction),
        };
        Cmd::None
    }
}

fn navigate(
    workspace: &mut Workspace,
    message: BrowseMessage,
) -> Result<Cmd, UpdateError> {
    Ok(workspace.browse.apply(message))
}

pub(crate) fn update(
    model: &mut Model,
    message: BrowseRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let len = model.playlist.tracks.len();
    model.workspace.browse.cursor = model.workspace.browse.cursor.resize(len);
    match message {
        BrowseRequest::ChordPrefix(prefix) => {
            model.workspace.chord_prefix = Some(prefix);
            Ok(Cmd::None)
        }
        BrowseRequest::CursorBy { rows } if len > 0 => {
            navigate(&mut model.workspace, BrowseMessage::CursorBy(rows))
        }
        BrowseRequest::Top => navigate(&mut model.workspace, BrowseMessage::Top),
        BrowseRequest::Bottom => navigate(&mut model.workspace, BrowseMessage::Bottom),
        BrowseRequest::CycleSort => Ok(cycle_sort(model)),
        BrowseRequest::ToggleFavorite => Ok(toggle_favorite(model)),
        BrowseRequest::CursorTo(index) if len > 0 => {
            navigate(&mut model.workspace, BrowseMessage::CursorTo(index))
        }
        BrowseRequest::PlaySelected => {
            let selected = model.workspace.browse.selected();
            crate::update::audio::jump_to(
                &mut crate::update::playback_parts(model),
                selected,
                now,
            )
        }
        BrowseRequest::PageBy(direction) if len > 0 => {
            let rows = model.workspace.visible_rows;
            navigate(&mut model.workspace, BrowseMessage::PageBy(rows, direction))
        }
        BrowseRequest::FullScan => Ok(full_scan(model)),
        BrowseRequest::Trash(track_index) => Ok(trash_track(model, track_index)),
        BrowseRequest::SavePlaylist(name) => {
            Ok(Effect::Library(LibraryCmd::SavePlaylist {
                name,
                tracks: model.playlist.tracks.clone(),
            })
            .into())
        }
        BrowseRequest::CursorBy { .. }
        | BrowseRequest::CursorTo(_)
        | BrowseRequest::PageBy(_) => Ok(Cmd::None),
    }
}

pub(crate) fn queue(
    model: &mut Model,
    message: QueueRequest,
) -> Result<Cmd, UpdateError> {
    let len = model.playlist.tracks.len();
    model.workspace.browse.cursor = model.workspace.browse.cursor.resize(len);
    let selected = selected_index(&model.playlist, &model.workspace);
    let queue = &mut model.queue;
    Ok(match (message, selected) {
        (QueueRequest::EnqueueTrack(index), _) => toggle_queued(queue, index),
        (_, None) => Cmd::None,
        (QueueRequest::Enqueue, Some(selected)) => toggle_queued(queue, selected),
        (QueueRequest::PlayNext, Some(selected)) => play_next(queue, selected),
        (QueueRequest::Dequeue, Some(selected)) => dequeue(queue, selected),
        (QueueRequest::MoveInQueue(direction), Some(selected)) => {
            move_in_queue(queue, selected, direction)
        }
    })
}

fn selected_index(playlist: &Playlist, workspace: &Workspace) -> Option<ViewIndex> {
    let selected = workspace.browse.selected();
    playlist.tracks.get(selected.get()).map(|_| selected)
}

fn full_scan(model: &mut Model) -> Cmd {
    match model.scan_status {
        ScanStatus::Idle => {
            model.scan_status = ScanStatus::Scanning;
            Effect::Library(LibraryCmd::Scan {
                music_dir: model.music_dir.clone(),
                revision: model.revisions.issue_scan(),
                mode: ScanMode::Full,
            })
            .into()
        }
        ScanStatus::Scanning | ScanStatus::Tagging { .. } => Cmd::None,
    }
}

fn toggle_favorite(model: &mut Model) -> Cmd {
    let selected = model.workspace.browse.selected();
    let Some(track) = model.playlist.tracks.get(selected.get()) else {
        return Cmd::None;
    };
    let path = track.path().to_path_buf();
    model.favorites.toggle(path);
    Cmd::Batch(vec![
        Effect::Library(LibraryCmd::SaveFavorites(model.favorites.clone())),
        Effect::Animate(Cue::FavoriteToggled),
    ])
}

fn toggle_queued(queue: &mut Vec<ViewIndex>, index: ViewIndex) -> Cmd {
    match queue.iter().position(|queued| *queued == index) {
        Some(position) => {
            queue.remove(position);
        }
        None => queue.push(index),
    }
    Cue::QueueChanged.into()
}

fn play_next(queue: &mut Vec<ViewIndex>, selected: ViewIndex) -> Cmd {
    queue.retain(|&queued| queued != selected);
    queue.insert(0, selected);
    Cmd::None
}

fn dequeue(queue: &mut Vec<ViewIndex>, selected: ViewIndex) -> Cmd {
    queue.retain(|&queued| queued != selected);
    Cmd::None
}

fn move_in_queue(
    queue: &mut [ViewIndex],
    selected: ViewIndex,
    direction: Direction,
) -> Cmd {
    let Some(index) = queue.iter().position(|&queued| queued == selected) else {
        return Cmd::None;
    };
    let neighbor = match direction {
        Direction::Previous => index.checked_sub(1),
        Direction::Next => index.checked_add(1).filter(|&next| next < queue.len()),
    };
    if let Some(neighbor) = neighbor {
        queue.swap(index, neighbor);
    }
    Cmd::None
}

fn cycle_sort(model: &mut Model) -> Cmd {
    model.workspace.browse.sort = cycled(model.workspace.browse.sort, Direction::Next);
    let current: Vec<&Track> = model
        .library
        .iter()
        .flat_map(Library::view_tracks)
        .map(|(_, track)| track.as_ref())
        .collect();
    let order = crate::domain::library::sort_indices(
        &current,
        model.workspace.browse.sort,
        &model.favorites,
    );
    let Some(library) = model.library.as_mut() else {
        return Cmd::None;
    };
    let old_view = std::mem::take(&mut library.view);
    library.view = order
        .into_iter()
        .filter_map(|position| old_view.get(position).copied())
        .collect();
    resync_playlist(ResyncParts {
        library,
        player: &model.player,
        playlist: &mut model.playlist,
        queue: &mut model.queue,
    });
    Cmd::None
}

pub(crate) struct ResyncParts<'a> {
    pub(crate) library: &'a mut Library,
    pub(crate) player: &'a Player,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) queue: &'a mut Vec<ViewIndex>,
}

fn trash_track(model: &mut Model, track_index: ViewIndex) -> Cmd {
    let Some(track) = model.playlist.tracks.get(track_index.get()).cloned() else {
        return Cmd::None;
    };
    let Some(library) = model.library.as_mut() else {
        return Cmd::None;
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
    resync_playlist(ResyncParts {
        library,
        player: &model.player,
        playlist: &mut model.playlist,
        queue: &mut model.queue,
    });
    Cmd::Batch(vec![
        Effect::Library(LibraryCmd::Trash(track.path().to_path_buf())),
        Effect::Animate(Cue::TrackDeleted),
    ])
}

pub(crate) fn resync_playlist(parts: ResyncParts<'_>) {
    let ResyncParts {
        library,
        player,
        playlist,
        queue,
    } = parts;
    let tracks: Vec<_> = library
        .view_tracks()
        .map(|(_, track)| Arc::clone(track))
        .collect();
    let anchor = player
        .current()
        .and_then(|track| index_of_path(track.path(), &tracks));
    remap_queue(&playlist.tracks, &tracks, queue);
    playlist.relist(tracks, anchor);
    playlist.play_order = std::mem::take(&mut playlist.play_order).without_order();
}

fn remap_queue(
    old_tracks: &[Arc<Track>],
    new_tracks: &[Arc<Track>],
    queue: &mut Vec<ViewIndex>,
) {
    *queue = queue
        .iter()
        .filter_map(|index| old_tracks.get(index.get()))
        .filter_map(|track| {
            new_tracks
                .iter()
                .position(|candidate| candidate.path() == track.path())
        })
        .map(ViewIndex::new)
        .collect();
}
