use std::sync::Arc;

use crate::{
    cmd::Cmd,
    domain::{
        Cursor,
        Direction,
        Model,
        Moment,
        Output,
        OutputDevice,
        Player,
        PlaylistIndex,
        Reply,
        Revision,
        Toast,
        Track,
        Transport,
        Workspace,
        playlist::{Playlist, RepeatMode},
    },
    message::{AudioError, AudioEvent},
    update::{
        error::UpdateError,
        machine::Machine,
        player::{self, Anchor, PlayerMessage, Stamp},
    },
};

pub(crate) fn update(
    model: &mut Model,
    event: AudioEvent,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match event {
        AudioEvent::Playhead(offset) => {
            let cmd = player::update_player(
                model,
                PlayerMessage::Playhead { offset, now },
                now,
            )?;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::TrackChanged => track_changed(model, now),
        AudioEvent::Ended => {
            let following = was_following(&model.workspace, &model.playlist);
            let cmd = ended(model, now)?;
            if following {
                follow_playback(&mut model.workspace, &model.playlist);
            }
            Ok(cmd)
        }
        AudioEvent::Loaded { total } => {
            let anchor = Anchor::at(model, now);
            let cmd = player::update_player(
                model,
                PlayerMessage::Loaded { total, anchor },
                now,
            )?;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::Error(failure) => error(model, failure, now),
        AudioEvent::DevicesListed(devices) => {
            model.settings.output_devices = devices;
            Ok(Cmd::None)
        }
        AudioEvent::DeviceFellBack(opened) => fell_back(model, &opened),
    }
}

fn fell_back(model: &mut Model, opened: &OutputDevice) -> Result<Cmd, UpdateError> {
    let requested = std::mem::replace(&mut model.settings.audio.device, opened.clone());
    let OutputDevice::Named(requested) = requested else {
        return Ok(Cmd::None);
    };
    if opened.named() == Some(&requested) {
        return Ok(Cmd::None);
    }
    let opened = opened.named().map_or_else(
        || "the system default".to_string(),
        crate::domain::DeviceName::to_string,
    );
    let told = format!("output device '{requested}' is gone — playing on {opened}");
    Ok(model
        .workspace
        .show(Toast::error(told), &mut model.revisions))
}

fn output_recovered(transport: &mut Transport) {
    if matches!(transport.output, Output::Lost { .. }) {
        transport.output = Output::Ready;
    }
}

fn error(
    model: &mut Model,
    error: AudioError,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let (told, lost) = match &error {
        AudioError::OutputLost { kind } => (
            output_lost_text(&model.player, &error),
            Some(Output::Lost { kind: *kind }),
        ),
        AudioError::Decode { .. }
        | AudioError::Device { .. }
        | AudioError::Stream { .. }
        | AudioError::Preload { .. }
        | AudioError::Seek { .. } => (error.to_string(), None),
    };
    let stopped =
        player::update_player(model, PlayerMessage::Error { error, now }, now)?;
    if let Some(lost) = lost {
        model.transport.output = lost;
    }
    let raised = model
        .workspace
        .show(Toast::error(told), &mut model.revisions);
    Ok(raised.then(stopped))
}

fn output_lost_text(player: &Player, error: &AudioError) -> String {
    match player {
        Player::Playing { .. } | Player::Paused { .. } => {
            "Output lost — paused".to_string()
        }
        Player::Loading { .. } | Player::Stopped => error.to_string(),
    }
}

fn cursor_to(playlist: &mut Playlist, dequeued: PlaylistIndex) {
    playlist.cursor = Cursor::with_len(playlist.tracks.len()).at(dequeued.get());
}

fn pop_queued_track(
    playlist: &mut Playlist,
    queue: &mut Vec<PlaylistIndex>,
) -> Option<Arc<Track>> {
    let index = *queue.first()?;
    queue.remove(0);
    let track = playlist.tracks.get(index.get()).cloned()?;
    cursor_to(playlist, index);
    Some(track)
}

pub(crate) fn next(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    let cmd = pop_queued_track(&mut model.playlist, &mut model.queue)
        .or_else(|| model.playlist.skip(Direction::Next).cloned())
        .map_or(Ok(Cmd::None), |track| start(model, track, now))?;
    follow_playback(&mut model.workspace, &model.playlist);
    Ok(cmd)
}

pub(crate) fn jump_to(
    model: &mut Model,
    index: PlaylistIndex,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    model
        .playlist
        .jump(index)
        .cloned()
        .map_or(Ok(Cmd::None), |track| start(model, track, now))
}

pub(crate) fn previous(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    let cmd = model
        .playlist
        .skip(Direction::Previous)
        .cloned()
        .map_or(Ok(Cmd::None), |track| start(model, track, now))?;
    follow_playback(&mut model.workspace, &model.playlist);
    Ok(cmd)
}

pub(crate) fn mark_fired(
    model: &mut Model,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    if matches!(revision.reply(model.revisions.mark), Reply::Stale)
        || !model.player.is_playing()
    {
        return Ok(Cmd::None);
    }
    let offset = model.player.position_at(now);
    let lookahead = player::lookahead(model, now);
    player::update_player(model, PlayerMessage::MarkReached { offset, lookahead }, now)
}

fn ended(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    let pick = successor(&model.playlist, &model.queue);
    let message = PlayerMessage::Ended {
        next: pick.track().cloned(),
        stamp: Stamp::issue(model, now),
    };
    let cmd = player::update_player(model, message, now)?;
    if pick.track().is_some() {
        model.transport.ab = None;
    }
    move_onto(&mut model.playlist, &mut model.queue, pick);
    Ok(cmd)
}

fn track_changed(model: &mut Model, now: Moment) -> Result<Cmd, UpdateError> {
    let following = was_following(&model.workspace, &model.playlist);
    let pick = match model.player.preloaded() {
        Some(committed) => Successor::Preloaded(Arc::clone(committed)),
        None if matches!(model.playlist.repeat, RepeatMode::One) => Successor::Nothing,
        None => successor(&model.playlist, &model.queue),
    };
    let message = PlayerMessage::TrackChanged {
        next: pick.track().cloned(),
        now,
    };
    let cmd = player::update_player(model, message, now)?;
    model.transport.ab = None;
    move_onto(&mut model.playlist, &mut model.queue, pick);
    if following {
        follow_playback(&mut model.workspace, &model.playlist);
    }
    Ok(cmd)
}

fn was_following(workspace: &Workspace, playlist: &Playlist) -> bool {
    playlist.playing_index() == Some(workspace.browse.selected())
}

fn follow_playback(workspace: &mut Workspace, playlist: &Playlist) {
    if let Some(anchor) = playlist.playing_index() {
        workspace.browse.cursor =
            Cursor::with_len(playlist.tracks.len()).at(anchor.get());
    }
}

pub(crate) enum Successor {
    Preloaded(Arc<Track>),
    Repeating(Arc<Track>),
    Queued {
        index: PlaylistIndex,
        track: Arc<Track>,
    },
    Following(Arc<Track>),
    Nothing,
}

impl Successor {
    pub(crate) fn track(&self) -> Option<&Arc<Track>> {
        match self {
            Successor::Preloaded(track)
            | Successor::Repeating(track)
            | Successor::Queued { track, .. }
            | Successor::Following(track) => Some(track),
            Successor::Nothing => None,
        }
    }
}

pub(crate) fn successor(playlist: &Playlist, queue: &[PlaylistIndex]) -> Successor {
    if matches!(playlist.repeat, RepeatMode::One) {
        return playlist
            .current()
            .cloned()
            .map_or(Successor::Nothing, Successor::Repeating);
    }
    if let Some(&index) = queue.first()
        && let Some(track) = playlist.tracks.get(index.get())
    {
        return Successor::Queued {
            index,
            track: Arc::clone(track),
        };
    }
    playlist
        .upcoming()
        .cloned()
        .map_or(Successor::Nothing, Successor::Following)
}

fn move_onto(playlist: &mut Playlist, queue: &mut Vec<PlaylistIndex>, pick: Successor) {
    match pick {
        Successor::Preloaded(committed) => {
            move_onto_preloaded(playlist, queue, &committed);
        }
        Successor::Queued { index, .. } => {
            queue.remove(0);
            cursor_to(playlist, index);
        }
        Successor::Following(_) => {
            playlist.skip(Direction::Next);
        }
        Successor::Repeating(_) | Successor::Nothing => {}
    }
}

fn move_onto_preloaded(
    playlist: &mut Playlist,
    queue: &mut Vec<PlaylistIndex>,
    committed: &Arc<Track>,
) {
    let committed_index = playlist
        .tracks
        .iter()
        .position(|playlist_track| {
            Arc::ptr_eq(playlist_track, committed)
                || playlist_track.path() == committed.path()
        })
        .map(PlaylistIndex::new);
    let Some(committed_index) = committed_index else {
        return;
    };
    queue.retain(|&queued| queued != committed_index);
    cursor_to(playlist, committed_index);
}

pub(crate) fn start(
    model: &mut Model,
    track: Arc<Track>,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let stamp = Stamp::issue(model, now);
    let cmd = model.player.update(PlayerMessage::Start { track, stamp })?;
    player::committed(&mut model.revisions, stamp.revision, &cmd);
    model.transport.ab = None;
    Ok(cmd)
}
