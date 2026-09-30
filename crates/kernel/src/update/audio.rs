use std::{sync::Arc, time::Duration};

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
        Settings,
        Toast,
        Track,
        Transport,
        Workspace,
        playlist::{Playlist, RepeatMode},
    },
    message::{AudioError, AudioEvent, WorkspaceRequest},
    update::{
        error::UpdateError,
        machine::Machine,
        player::{self, Anchor, PlayerMessage},
    },
};

pub(crate) fn update(
    model: &mut Model,
    event: AudioEvent,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match event {
        AudioEvent::Playhead(offset) => {
            let cmd = positioned(model, offset, now)?;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::TrackChanged => track_changed(model, now),
        AudioEvent::Ended => {
            let following = was_following(&model.workspace, &model.playlist);
            let cmd = ended(model, now)?;
            follow_if(following, &mut model.workspace, &model.playlist);
            Ok(cmd)
        }
        AudioEvent::Loaded { total } => {
            let anchor = Anchor {
                now,
                speed: model.transport.speed,
            };
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
        AudioEvent::DeviceFellBack(opened) => fell_back(
            FallbackParts {
                settings: &mut model.settings,
                workspace: &mut model.workspace,
            },
            &opened,
        ),
    }
}

struct FallbackParts<'a> {
    settings: &'a mut Settings,
    workspace: &'a mut Workspace,
}

fn fell_back(
    slices: FallbackParts<'_>,
    opened: &OutputDevice,
) -> Result<Cmd, UpdateError> {
    let FallbackParts {
        settings,
        workspace,
    } = slices;
    let requested = std::mem::replace(&mut settings.output_device, opened.clone());
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
    Ok(workspace.update(WorkspaceRequest::ShowToast(Toast::error(told)))?)
}

fn output_recovered(transport: &mut Transport) {
    if matches!(transport.output, Output::Lost { .. }) {
        transport.output = Output::Ready;
    }
}

fn error(
    model: &mut Model,
    failure: AudioError,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let (told, lost) = match &failure {
        AudioError::OutputLost { kind } => (
            output_lost_text(&model.player, &failure),
            Some(Output::Lost { kind: *kind }),
        ),
        AudioError::Decode { .. }
        | AudioError::Device { .. }
        | AudioError::Stream { .. }
        | AudioError::Preload { .. }
        | AudioError::Seek { .. } => (failure.to_string(), None),
    };
    let stopped =
        player::update_player(model, PlayerMessage::Error { failure, now }, now)?;
    if let Some(lost) = lost {
        model.transport.output = lost;
    }
    let raised = model
        .workspace
        .update(WorkspaceRequest::ShowToast(Toast::error(told)))?;
    Ok(raised.then(stopped))
}

fn output_lost_text(player: &Player, failure: &AudioError) -> String {
    match player {
        Player::Playing { .. } | Player::Paused { .. } => {
            "Output lost — paused".to_string()
        }
        Player::Loading { .. } | Player::Stopped => failure.to_string(),
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

pub(crate) fn next(model: &mut Model) -> Result<Cmd, UpdateError> {
    let cmd = pop_queued_track(&mut model.playlist, &mut model.queue)
        .or_else(|| model.playlist.skip(Direction::Next).cloned())
        .map_or(Ok(Cmd::None), |track| {
            start(&mut model.transport, &mut model.player, track)
        })?;
    follow_if(Follow::Playback, &mut model.workspace, &model.playlist);
    Ok(cmd)
}

pub(crate) fn previous(model: &mut Model) -> Result<Cmd, UpdateError> {
    let cmd = model
        .playlist
        .skip(Direction::Previous)
        .cloned()
        .map_or(Ok(Cmd::None), |track| {
            start(&mut model.transport, &mut model.player, track)
        })?;
    follow_if(Follow::Playback, &mut model.workspace, &model.playlist);
    Ok(cmd)
}

fn positioned(
    model: &mut Model,
    offset: Duration,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    player::update_player(model, PlayerMessage::Playhead { offset, now }, now)
}

pub(crate) fn mark_fired(
    model: &mut Model,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    if let Reply::Stale = revision.reply(model.revisions.mark) {
        return Ok(Cmd::None);
    }
    if !model.player.is_playing() {
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
    follow_if(following, &mut model.workspace, &model.playlist);
    Ok(cmd)
}

fn was_following(workspace: &Workspace, playlist: &Playlist) -> Follow {
    if playlist.playing_index() == Some(workspace.browse.selected()) {
        Follow::Playback
    } else {
        Follow::Nothing
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Follow {
    Playback,
    Nothing,
}

fn follow_if(follow: Follow, workspace: &mut Workspace, playlist: &Playlist) {
    match (follow, playlist.playing_index()) {
        (Follow::Playback, Some(anchor)) => {
            workspace.browse.cursor =
                Cursor::with_len(playlist.tracks.len()).at(anchor.get());
        }
        (Follow::Playback, None) | (Follow::Nothing, _) => {}
    }
}

enum Successor {
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
    fn track(&self) -> Option<&Arc<Track>> {
        match self {
            Successor::Preloaded(track)
            | Successor::Repeating(track)
            | Successor::Queued { track, .. }
            | Successor::Following(track) => Some(track),
            Successor::Nothing => None,
        }
    }
}

fn successor(playlist: &Playlist, queue: &[PlaylistIndex]) -> Successor {
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

pub(crate) fn track_duration(track: &Arc<Track>) -> Duration {
    track.duration().unwrap_or_default()
}

pub(crate) fn start(
    transport: &mut Transport,
    player: &mut Player,
    track: Arc<Track>,
) -> Result<Cmd, UpdateError> {
    let cmd = player.update(PlayerMessage::Start { track })?;
    transport.ab = None;
    Ok(cmd)
}
