use std::{sync::Arc, time::Duration};

use crate::{
    cmd::Cmd,
    domain::{
        AbLoop,
        Cursor,
        CursorDirection,
        Model,
        Output,
        Player,
        PlaylistIndex,
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
        player::{Lookahead, PlayerMessage},
        rejection::Rejection,
    },
};

pub(super) fn audio(model: &mut Model, event: AudioEvent) -> Result<Cmd, Rejection> {
    match event {
        AudioEvent::Position(at) => {
            let listened = model.player.listened(at);
            let cmd = position(model, at)?;
            model.workspace.played_for += listened;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::TrackChanged => {
            let cmd = track_changed(model)?;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::Ended => {
            let following = was_following(&model.workspace, &model.playlist);
            let cmd = ended(EndLanding {
                playlist: &mut model.playlist,
                queue: &mut model.queue,
                player: &mut model.player,
                transport: &mut model.transport,
            })?;
            follow_if(following, &mut model.workspace, &model.playlist);
            Ok(cmd)
        }
        AudioEvent::Loaded { total } => {
            let cmd = model.player.update(PlayerMessage::Loaded { total })?;
            output_recovered(&mut model.transport);
            Ok(cmd)
        }
        AudioEvent::Error(failure) => error(
            FailureLanding {
                workspace: &mut model.workspace,
                player: &mut model.player,
                transport: &mut model.transport,
            },
            failure,
        ),
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
        AudioEvent::OutputRouteChanged => route_changed(model),
    }
}

fn route_changed(model: &mut Model) -> Result<Cmd, Rejection> {
    if !model.player.is_playing() {
        return Ok(Cmd::None);
    }
    let current = model.playlist.current().cloned();
    let paused = model.player.update(PlayerMessage::Toggle {
        current,
        volume: model.transport.volume,
    })?;
    let raised = model
        .workspace
        .update(WorkspaceRequest::ShowToast(Toast::info(
            "Output changed — paused".to_string(),
        )))?;
    Ok(raised.then(paused))
}

struct FallbackLanding<'a> {
    settings: &'a mut Settings,
    workspace: &'a mut Workspace,
}

fn fell_back(
    slices: FallbackLanding<'_>,
    opened: Option<String>,
) -> Result<Cmd, Rejection> {
    let FallbackLanding {
        settings,
        workspace,
    } = slices;
    let requested = std::mem::replace(&mut settings.output_device, opened.clone());
    let Some(requested) = requested.filter(|name| Some(name) != opened.as_ref()) else {
        return Ok(Cmd::None);
    };
    let opened = opened.unwrap_or_else(|| "the system default".to_string());
    let told = format!("output device '{requested}' is gone — playing on {opened}");
    Ok(workspace.update(WorkspaceRequest::ShowToast(Toast::error(told)))?)
}

fn output_recovered(transport: &mut Transport) {
    if matches!(transport.output, Output::Lost { .. }) {
        transport.output = Output::Ready;
    }
}

struct FailureLanding<'a> {
    workspace: &'a mut Workspace,
    player: &'a mut Player,
    transport: &'a mut Transport,
}

fn error(slices: FailureLanding<'_>, failure: AudioFailure) -> Result<Cmd, Rejection> {
    let FailureLanding {
        workspace,
        player,
        transport,
    } = slices;
    let (told, lost) = match &failure {
        AudioFailure::OutputLost { reason } => (
            output_lost_text(player, &failure),
            Some(Output::Lost {
                reason: reason.clone(),
            }),
        ),
        AudioFailure::Decode { .. }
        | AudioFailure::Device { .. }
        | AudioFailure::Stream { .. }
        | AudioFailure::Preload { .. }
        | AudioFailure::Seek { .. } => (failure.to_string(), None),
    };
    let stopped = player.update(PlayerMessage::Error(failure))?;
    if let Some(lost) = lost {
        transport.output = lost;
    }
    let raised = workspace.update(WorkspaceRequest::ShowToast(Toast::error(told)))?;
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

pub(super) fn next(model: &mut Model) -> Result<Cmd, Rejection> {
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

pub(super) fn previous(model: &mut Model) -> Result<Cmd, Rejection> {
    let cmd = playlist::skip(&mut model.playlist, CursorDirection::Backward)
        .cloned()
        .map_or(Ok(Cmd::None), |track| {
            start(&mut model.transport, &mut model.player, track)
        })?;
    follow_if(Follow::Playback, &mut model.workspace, &model.playlist);
    Ok(cmd)
}

fn position(model: &mut Model, at: Duration) -> Result<Cmd, Rejection> {
    let ab_loop = match model.transport.ab {
        Some(AbLoop::Full { a, b }) => Some((a, b)),
        Some(AbLoop::AOnly(_)) | None => None,
    };
    let lookahead = Lookahead {
        preload_lead: model.transport.preload_lead,
        ab_loop,
        next: next_track(&model.playlist, &model.queue),
    };
    Ok(model
        .player
        .update(PlayerMessage::Position { at, lookahead })?)
}

struct EndLanding<'a> {
    playlist: &'a mut Playlist,
    queue: &'a mut Vec<PlaylistIndex>,
    player: &'a mut Player,
    transport: &'a mut Transport,
}

fn ended(slices: EndLanding<'_>) -> Result<Cmd, Rejection> {
    let EndLanding {
        playlist,
        queue,
        player,
        transport,
    } = slices;
    let pick = successor(playlist, queue);
    let message = PlayerMessage::Ended {
        next: pick.track().cloned(),
        volume: transport.volume,
    };
    let cmd = player.update(message)?;
    if pick.track().is_some() {
        transport.ab = None;
    }
    move_onto(playlist, queue, pick);
    Ok(cmd)
}

fn track_changed(model: &mut Model) -> Result<Cmd, Rejection> {
    let following = was_following(&model.workspace, &model.playlist);
    let pick = match model.player.preloaded() {
        Some(committed) => Successor::Preloaded(Arc::clone(committed)),
        None if matches!(model.playlist.repeat, RepeatMode::One) => Successor::Nothing,
        None => successor(&model.playlist, &model.queue),
    };
    let message = PlayerMessage::TrackChanged {
        next: pick.track().cloned(),
    };
    let cmd = model.player.update(message)?;
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

pub(super) fn track_duration(track: &Arc<Track>) -> Duration {
    track.duration().unwrap_or_default()
}

fn next_track(playlist: &Playlist, queue: &[PlaylistIndex]) -> Option<Arc<Track>> {
    if matches!(playlist.repeat, RepeatMode::One) {
        return playlist.current().cloned();
    }
    if let Some(index) = queue.first() {
        return playlist.tracks.get(index.get()).cloned();
    }
    playlist::upcoming(playlist).cloned()
}

pub(super) fn start(
    transport: &mut Transport,
    player: &mut Player,
    track: Arc<Track>,
) -> Result<Cmd, Rejection> {
    let cmd = player.update(PlayerMessage::Start {
        track,
        volume: transport.volume,
    })?;
    transport.ab = None;
    Ok(cmd)
}
