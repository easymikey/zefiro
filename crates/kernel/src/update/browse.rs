use std::{path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd},
    domain::{
        Browse,
        Cursor,
        CursorDirection,
        Model,
        Nudge,
        Player,
        PlaylistIndex,
        Revision,
        ScanStatus,
        Track,
        TrackIndex,
        Workspace,
        cycled,
        library::Library,
        playlist::{self, Playlist},
    },
    message::BrowseRequest,
    update::{
        machine::{Machine, Never, Rejected},
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy)]
pub enum BrowseMessage {
    SelectBy(isize),
    Top,
    Bottom,
    CursorTo(usize),
    PageBy(usize, Nudge),
}

impl Machine for Browse {
    type Message = BrowseMessage;
    type Rejection = Never;
    type Effect = ();

    fn transition(
        mut self,
        message: BrowseMessage,
    ) -> Result<(Self, ()), Rejected<Self>> {
        self.cursor = match message {
            BrowseMessage::SelectBy(delta) => self.cursor.step(delta),
            BrowseMessage::Top => self.cursor.first(),
            BrowseMessage::Bottom => self.cursor.last(),
            BrowseMessage::CursorTo(index) => {
                Cursor::with_len(self.cursor.len()).at(index)
            }
            BrowseMessage::PageBy(rows, nudge) => {
                let direction = match nudge {
                    Nudge::Up => CursorDirection::Backward,
                    Nudge::Down => CursorDirection::Forward,
                };
                self.cursor.page(rows, direction)
            }
        };
        Ok((self, ()))
    }
}

fn navigate(
    workspace: &mut Workspace,
    message: BrowseMessage,
) -> Result<Cmd, Rejection> {
    workspace.browse.update(message)?;
    Ok(Cmd::None)
}

pub(crate) fn update(
    model: &mut Model,
    message: BrowseRequest,
) -> Result<Cmd, Rejection> {
    let len = model.playlist.tracks.len();
    model.workspace.browse.cursor = model.workspace.browse.cursor.resize(len);
    match message {
        BrowseRequest::ChordPrefix(prefix) => {
            model.workspace.chord = Some(prefix);
            Ok(Cmd::None)
        }
        BrowseRequest::CursorBy(delta) if len > 0 => isize::try_from(delta.get())
            .map_or(Ok(Cmd::None), |delta| {
                navigate(&mut model.workspace, BrowseMessage::SelectBy(delta))
            }),
        BrowseRequest::Top => navigate(&mut model.workspace, BrowseMessage::Top),
        BrowseRequest::Bottom => navigate(&mut model.workspace, BrowseMessage::Bottom),
        BrowseRequest::CycleSort => Ok(cycle_sort(model)),
        BrowseRequest::ToggleFavorite => Ok(toggle_favorite(model)),
        BrowseRequest::CursorTo(index) if len > 0 => {
            navigate(&mut model.workspace, BrowseMessage::CursorTo(index.get()))
        }
        BrowseRequest::Enqueue => Ok(enqueue_selected(model)),
        BrowseRequest::EnqueueTrack(index) => {
            Ok(toggle_queued(&mut model.queue, index))
        }
        BrowseRequest::PlayNext => Ok(play_next_selected(model)),
        BrowseRequest::Dequeue => Ok(dequeue_selected(model)),
        BrowseRequest::MoveInQueue(direction) => Ok(move_in_queue(model, direction)),
        BrowseRequest::PlaySelected => play_selected(model),
        BrowseRequest::PageBy(nudge) if len > 0 => {
            let rows = model.workspace.visible_rows;
            navigate(&mut model.workspace, BrowseMessage::PageBy(rows, nudge))
        }
        BrowseRequest::Rescan => Ok(rescan(&mut model.scan_status, &model.music_dir)),
        BrowseRequest::Trash(track_index) => Ok(trash_track(model, track_index)),
        BrowseRequest::SavePlaylist(name) => {
            Ok(Effect::Library(LibraryCmd::SavePlaylist {
                name,
                tracks: model.playlist.tracks.clone(),
            })
            .into())
        }
        BrowseRequest::CursorBy(_)
        | BrowseRequest::CursorTo(_)
        | BrowseRequest::PageBy(_) => Ok(Cmd::None),
    }
}

fn selected_index(playlist: &Playlist, workspace: &Workspace) -> Option<PlaylistIndex> {
    let selected = workspace.browse.selected();
    playlist.tracks.get(selected.get()).map(|_| selected)
}

fn rescan(scan_status: &mut ScanStatus, music_dir: &Path) -> Cmd {
    match scan_status {
        ScanStatus::Idle => {
            *scan_status = ScanStatus::Scanning;
            Effect::Library(LibraryCmd::Rescan {
                root: music_dir.to_path_buf(),
                revision: Revision::UNSTAMPED,
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
        Effect::Library(LibraryCmd::SaveFavorites(
            model.favorites.clone().into_inner(),
        )),
        Effect::Animate(Cue::FavoriteToggled),
    ])
}

fn enqueue_selected(model: &mut Model) -> Cmd {
    let Some(selected) = selected_index(&model.playlist, &model.workspace) else {
        return Cmd::None;
    };
    toggle_queued(&mut model.queue, selected)
}

fn toggle_queued(queue: &mut Vec<PlaylistIndex>, index: PlaylistIndex) -> Cmd {
    match queue.iter().position(|queued| *queued == index) {
        Some(position) => {
            queue.remove(position);
        }
        None => queue.push(index),
    }
    Cue::QueueChanged.into()
}

fn play_next_selected(model: &mut Model) -> Cmd {
    let Some(selected) = selected_index(&model.playlist, &model.workspace) else {
        return Cmd::None;
    };
    model.queue.retain(|&queued| queued != selected);
    model.queue.insert(0, selected);
    Cmd::None
}

fn dequeue_selected(model: &mut Model) -> Cmd {
    let Some(selected) = selected_index(&model.playlist, &model.workspace) else {
        return Cmd::None;
    };
    model.queue.retain(|&queued| queued != selected);
    Cmd::None
}

fn move_in_queue(model: &mut Model, direction: Nudge) -> Cmd {
    let Some(selected) = selected_index(&model.playlist, &model.workspace) else {
        return Cmd::None;
    };
    let Some(index) = model.queue.iter().position(|&queued| queued == selected) else {
        return Cmd::None;
    };
    let neighbor = match direction {
        Nudge::Up => index.checked_sub(1),
        Nudge::Down => index
            .checked_add(1)
            .filter(|&next| next < model.queue.len()),
    };
    if let Some(neighbor) = neighbor {
        model.queue.swap(index, neighbor);
    }
    Cmd::None
}

fn play_selected(model: &mut Model) -> Result<Cmd, Rejection> {
    playlist::jump(&mut model.playlist, model.workspace.browse.selected())
        .cloned()
        .map_or(Ok(Cmd::None), |track| {
            crate::update::audio::start(&mut model.transport, &mut model.player, track)
        })
}

fn cycle_sort(model: &mut Model) -> Cmd {
    model.workspace.browse.sort = cycled(model.workspace.browse.sort, Nudge::Up);
    let current: Vec<&Track> = model
        .library
        .view_tracks()
        .map(|(_, track)| track.as_ref())
        .collect();
    let order = crate::domain::library::sort_indices(
        &current,
        model.workspace.browse.sort,
        &model.favorites,
    );
    let Some(library) = model.library.ready_mut() else {
        return Cmd::None;
    };
    let old_view = std::mem::take(&mut library.view);
    library.view = order
        .into_iter()
        .filter_map(|position| old_view.get(position).copied())
        .collect();
    resync_playlist(TrashRequest {
        library,
        player: &model.player,
        playlist: &mut model.playlist,
        queue: &mut model.queue,
    });
    Cmd::None
}

pub(crate) struct TrashRequest<'a> {
    pub(crate) library: &'a mut Library,
    pub(crate) player: &'a Player,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) queue: &'a mut Vec<PlaylistIndex>,
}

fn trash_track(model: &mut Model, track_index: PlaylistIndex) -> Cmd {
    let Some(track) = model.playlist.tracks.get(track_index.get()).cloned() else {
        return Cmd::None;
    };
    let Some(library) = model.library.ready_mut() else {
        return Cmd::None;
    };
    let removed_position = library.all.iter().position(|t| t.path() == track.path());
    library.all.retain(|t| t.path() != track.path());
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
    resync_playlist(TrashRequest {
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

pub(crate) fn resync_playlist(slices: TrashRequest<'_>) {
    let TrashRequest {
        library,
        player,
        playlist,
        queue,
    } = slices;
    let tracks: Vec<_> = library
        .view_tracks()
        .map(|(_, track)| Arc::clone(track))
        .collect();
    let playing_path = player.current().map(|track| track.path().to_path_buf());
    let anchor = playlist::anchor_of(playing_path.as_deref(), &tracks);
    remap_queue(&playlist.tracks, &tracks, queue);
    playlist::relist(playlist, tracks, anchor);
    playlist.play_order = std::mem::take(&mut playlist.play_order).without_order();
}

fn remap_queue(
    old_tracks: &[Arc<Track>],
    new_tracks: &[Arc<Track>],
    queue: &mut Vec<PlaylistIndex>,
) {
    *queue = queue
        .iter()
        .filter_map(|index| old_tracks.get(index.get()))
        .filter_map(|track| {
            new_tracks
                .iter()
                .position(|candidate| candidate.path() == track.path())
        })
        .map(PlaylistIndex::new)
        .collect();
}
