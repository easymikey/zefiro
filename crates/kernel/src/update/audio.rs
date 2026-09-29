use std::{sync::Arc, time::Duration};

use crate::{
    cmd::Cmd,
    domain::{
        Cursor,
        CursorDirection,
        DeviceName,
        Model,
        Moment,
        Output,
        Player,
        PlaylistIndex,
        Reply,
        Revision,
        Settings,
        Toast,
        Track,
        Transport,
        Workspace,
        playlist::{self, Playlist, RepeatMode},
    },
    message::{AudioEvent, AudioFailure, WorkspaceRequest},
    update::{
        machine::Machine,
        player::{self, Anchor, PlayerMessage},
        rejection::Rejection,
    },
};

pub(crate) fn audio(
    model: &mut Model,
    event: AudioEvent,
    now: Moment,
) -> Result<Cmd, Rejection> {
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
            let cmd =
                player::account(model, PlayerMessage::Loaded { total, anchor }, now)?;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::Error(failure) => error(model, failure, now),
        AudioEvent::Rejected(rejection) => Err(Rejection::Engine(rejection)),
        AudioEvent::DevicesLoaded(devices) => {
            model.settings.output_devices = devices;
            Ok(Cmd::None)
        }
        AudioEvent::DeviceFellBack(opened) => fell_back(
            FallbackLanding {
                settings: &mut model.settings,
                workspace: &mut model.workspace,
            },
            opened,
        ),
    }
}

struct FallbackLanding<'a> {
    settings: &'a mut Settings,
    workspace: &'a mut Workspace,
}

fn fell_back(
    slices: FallbackLanding<'_>,
    opened: Option<DeviceName>,
) -> Result<Cmd, Rejection> {
    let FallbackLanding {
        settings,
        workspace,
    } = slices;
    let requested = std::mem::replace(&mut settings.output_device, opened.clone());
    let Some(requested) = requested.filter(|name| Some(name) != opened.as_ref()) else {
        return Ok(Cmd::None);
    };
    let opened = opened
        .map_or_else(|| "the system default".to_string(), |name| name.to_string());
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
    failure: AudioFailure,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let (told, lost) = match &failure {
        AudioFailure::OutputLost { fault } => (
            output_lost_text(&model.player, &failure),
            Some(Output::Lost { fault: *fault }),
        ),
        AudioFailure::Decode { .. }
        | AudioFailure::Device { .. }
        | AudioFailure::Stream { .. }
        | AudioFailure::Preload { .. }
        | AudioFailure::Seek { .. } => (failure.to_string(), None),
    };
    let stopped = player::account(model, PlayerMessage::Error { failure, now }, now)?;
    if let Some(lost) = lost {
        model.transport.output = lost;
    }
    let raised = model
        .workspace
        .update(WorkspaceRequest::ShowToast(Toast::error(told)))?;
    Ok(raised.then(stopped))
}

fn output_lost_text(player: &Player, failure: &AudioFailure) -> String {
    match player {
        Player::Playing { .. } | Player::Paused { .. } => {
            "Output lost — paused".to_string()
        }
        Player::Loading { .. } | Player::Stopped => failure.to_string(),
    }
}

fn cursor_to(playlist: &mut Playlist, dequeued: PlaylistIndex) {
    playlist.at = Cursor::with_len(playlist.tracks.len()).at(dequeued.get());
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

pub(crate) fn next(model: &mut Model) -> Result<Cmd, Rejection> {
    let cmd = pop_queued_track(&mut model.playlist, &mut model.queue)
        .or_else(|| {
            playlist::skip(&mut model.playlist, CursorDirection::Forward).cloned()
        })
        .map_or(Ok(Cmd::None), |track| {
            start(&mut model.transport, &mut model.player, track)
        })?;
    follow_if(Follow::Playback, &mut model.workspace, &model.playlist);
    Ok(cmd)
}

pub(crate) fn previous(model: &mut Model) -> Result<Cmd, Rejection> {
    let cmd = playlist::skip(&mut model.playlist, CursorDirection::Backward)
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
) -> Result<Cmd, Rejection> {
    player::account(model, PlayerMessage::Reported { offset, now }, now)
}

pub(crate) fn mark_fired(
    model: &mut Model,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Rejection> {
    if let Reply::Stale = revision.reply(model.mark_generation) {
        return Ok(Cmd::None);
    }
    if !model.player.is_playing() {
        return Ok(Cmd::None);
    }
    let offset = model.player.position_at(now);
    let lookahead = player::lookahead(model, now);
    player::account(model, PlayerMessage::Playhead { offset, lookahead }, now)
}

fn ended(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
    let pick = successor(&model.playlist, &model.queue);
    let message = PlayerMessage::Ended {
        next: pick.track().cloned(),
    };
    let cmd = player::account(model, message, now)?;
    if pick.track().is_some() {
        model.transport.ab = None;
    }
    move_onto(&mut model.playlist, &mut model.queue, pick);
    Ok(cmd)
}

fn track_changed(model: &mut Model, now: Moment) -> Result<Cmd, Rejection> {
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
    let cmd = player::account(model, message, now)?;
    model.transport.ab = None;
    move_onto(&mut model.playlist, &mut model.queue, pick);
    follow_if(following, &mut model.workspace, &model.playlist);
    Ok(cmd)
}

fn was_following(workspace: &Workspace, playlist: &Playlist) -> Follow {
    if playlist.anchor() == Some(workspace.browse.selected()) {
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
    match (follow, playlist.anchor()) {
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
    playlist::upcoming(playlist)
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
            playlist::skip(playlist, CursorDirection::Forward);
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
) -> Result<Cmd, Rejection> {
    let cmd = player.update(PlayerMessage::Start { track })?;
    transport.ab = None;
    Ok(cmd)
}
