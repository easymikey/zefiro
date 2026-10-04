use std::sync::Arc;

use crate::{
    cmd::Cmd,
    domain::{
        Cursor,
        Direction,
        Freshness,
        Moment,
        Output,
        OutputDevice,
        Player,
        Revision,
        Toast,
        Track,
        TrackRef,
        Transport,
        ViewIndex,
        Workspace,
        playlist::{Playlist, RepeatMode},
    },
    message::{AudioError, AudioEvent},
    update::{
        machine::{Machine, Unhandled},
        player::{self, Anchor, PlaybackParts, PlayerMessage, Stamp},
    },
};

pub(crate) fn update(
    playback: &mut PlaybackParts<'_>,
    event: AudioEvent,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match event {
        AudioEvent::Playhead(offset) => {
            let cmd = player::update_player(
                playback,
                PlayerMessage::Playhead { offset, now },
                now,
            )?;
            output_recovered(playback.transport);
            Ok(cmd)
        }
        AudioEvent::TrackChanged => track_changed(playback, now),
        AudioEvent::Ended => {
            let following = was_following(playback.workspace, playback.playlist);
            let cmd = ended(playback, now)?;
            if following {
                follow_playback(playback.workspace, playback.playlist);
            }
            Ok(cmd)
        }
        AudioEvent::Loaded(total) => {
            let anchor = Anchor::at(playback.transport, now);
            let cmd = player::update_player(
                playback,
                PlayerMessage::Loaded { total, anchor },
                now,
            )?;
            output_recovered(playback.transport);
            Ok(cmd)
        }
        AudioEvent::Error(failure) => error(playback, failure, now),
        AudioEvent::DevicesListed(devices) => {
            playback.settings.output_devices = devices;
            Ok(Cmd::none())
        }
        AudioEvent::DeviceFellBack(opened) => fell_back(playback, &opened),
    }
}

fn fell_back(
    playback: &mut PlaybackParts<'_>,
    opened: &OutputDevice,
) -> Result<Cmd, Unhandled> {
    let requested =
        std::mem::replace(&mut playback.settings.audio.device, opened.clone());
    let OutputDevice::Named(requested) = requested else {
        return Ok(Cmd::none());
    };
    if opened.named() == Some(&requested) {
        return Ok(Cmd::none());
    }
    let opened = opened.named().map_or_else(
        || "the system default".to_string(),
        crate::domain::DeviceName::to_string,
    );
    let told = format!("output device '{requested}' is gone — playing on {opened}");
    Ok(playback.workspace.show(
        Toast::error("Output device lost").with_text(told),
        playback.revisions,
    ))
}

fn output_recovered(transport: &mut Transport) {
    if matches!(transport.output, Output::Lost(..)) {
        transport.output = Output::Ready;
    }
}

fn error(
    playback: &mut PlaybackParts<'_>,
    error: AudioError,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let (told, lost) = match &error {
        AudioError::OutputLost(kind) => (
            output_lost_text(playback.player, &error),
            Some(Output::Lost(*kind)),
        ),
        AudioError::Decode { .. }
        | AudioError::Device { .. }
        | AudioError::Stream { .. }
        | AudioError::Preload { .. }
        | AudioError::Seek { .. } => (error.to_string(), None),
    };
    let stopped =
        player::update_player(playback, PlayerMessage::Error { error, now }, now)?;
    if let Some(lost) = lost {
        playback.transport.output = lost;
    }
    let raised = playback.workspace.show(
        Toast::error("Audio error").with_text(told),
        playback.revisions,
    );
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

fn cursor_to(playlist: &mut Playlist, dequeued: ViewIndex) {
    playlist.cursor = Cursor::with_len(playlist.tracks.len()).at(dequeued.get());
}

fn first_queued<'a>(
    playlist: &'a Playlist,
    queue: &[TrackRef],
) -> Option<(usize, ViewIndex, &'a Arc<Track>)> {
    queue.iter().enumerate().find_map(|(position, source)| {
        let index = playlist.index_of(source)?;
        let track = playlist.tracks.get(index.get())?;
        Some((position, index, track))
    })
}

fn pop_queued_track(
    playlist: &mut Playlist,
    queue: &mut Vec<TrackRef>,
) -> Option<Arc<Track>> {
    let (position, index, track) = first_queued(playlist, queue)
        .map(|(position, index, track)| (position, index, Arc::clone(track)))?;
    queue.drain(..=position);
    cursor_to(playlist, index);
    Some(track)
}

pub(crate) fn next(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let cmd = pop_queued_track(playback.playlist, playback.queue)
        .or_else(|| playback.playlist.skip(Direction::Next).cloned())
        .map_or(Ok(Cmd::none()), |track| start(playback, track, now))?;
    follow_playback(playback.workspace, playback.playlist);
    Ok(cmd)
}

pub(crate) fn jump_to(
    playback: &mut PlaybackParts<'_>,
    index: ViewIndex,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    playback
        .playlist
        .jump(index)
        .cloned()
        .map_or(Ok(Cmd::none()), |track| start(playback, track, now))
}

pub(crate) fn previous(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let cmd = playback
        .playlist
        .skip(Direction::Previous)
        .cloned()
        .map_or(Ok(Cmd::none()), |track| start(playback, track, now))?;
    follow_playback(playback.workspace, playback.playlist);
    Ok(cmd)
}

pub(crate) fn lookahead_fired(
    playback: &mut PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if matches!(
        revision.freshness(playback.revisions.lookahead),
        Freshness::Stale
    ) || !playback.player.is_playing()
    {
        return Ok(Cmd::none());
    }
    let offset = playback.player.position_at(now);
    let lookahead = player::lookahead(playback, now);
    player::update_player(
        playback,
        PlayerMessage::LookaheadReached { offset, lookahead },
        now,
    )
}

fn ended(playback: &mut PlaybackParts<'_>, now: Moment) -> Result<Cmd, Unhandled> {
    let pick = successor(playback.playlist, playback.queue);
    let message = PlayerMessage::Ended {
        next: pick.track().cloned(),
        stamp: Stamp::pending(playback.transport, playback.revisions, now),
    };
    let cmd = player::update_player(playback, message, now)?;
    if pick.track().is_some() {
        playback.transport.ab_loop = None;
    }
    move_onto(playback.playlist, playback.queue, pick);
    Ok(cmd)
}

fn track_changed(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let following = was_following(playback.workspace, playback.playlist);
    let pick = match playback.player.preloaded() {
        Some(committed) => Successor::Preloaded(Arc::clone(committed)),
        None if matches!(playback.playlist.repeat, RepeatMode::One) => {
            Successor::Nothing
        }
        None => successor(playback.playlist, playback.queue),
    };
    let message = PlayerMessage::TrackChanged {
        next: pick.track().cloned(),
        now,
    };
    let cmd = player::update_player(playback, message, now)?;
    playback.transport.ab_loop = None;
    move_onto(playback.playlist, playback.queue, pick);
    if following {
        follow_playback(playback.workspace, playback.playlist);
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
        position: usize,
        index: ViewIndex,
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

pub(crate) fn successor(playlist: &Playlist, queue: &[TrackRef]) -> Successor {
    if matches!(playlist.repeat, RepeatMode::One) {
        return playlist
            .current()
            .cloned()
            .map_or(Successor::Nothing, Successor::Repeating);
    }
    if let Some((position, index, track)) = first_queued(playlist, queue) {
        return Successor::Queued {
            position,
            index,
            track: Arc::clone(track),
        };
    }
    playlist
        .upcoming()
        .cloned()
        .map_or(Successor::Nothing, Successor::Following)
}

fn move_onto(playlist: &mut Playlist, queue: &mut Vec<TrackRef>, pick: Successor) {
    match pick {
        Successor::Preloaded(committed) => {
            move_onto_preloaded(playlist, queue, &committed);
        }
        Successor::Queued {
            position, index, ..
        } => {
            queue.drain(..=position);
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
    queue: &mut Vec<TrackRef>,
    committed: &Arc<Track>,
) {
    let committed_index = playlist
        .tracks
        .iter()
        .position(|playlist_track| {
            Arc::ptr_eq(playlist_track, committed)
                || playlist_track.path() == committed.path()
        })
        .map(ViewIndex::new);
    let Some(committed_index) = committed_index else {
        return;
    };
    queue.retain(|queued| queued != committed.source());
    cursor_to(playlist, committed_index);
}

pub(crate) fn start(
    playback: &mut PlaybackParts<'_>,
    track: Arc<Track>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let stamp = Stamp::pending(playback.transport, playback.revisions, now);
    let cmd = playback
        .player
        .transition(PlayerMessage::Start { track, stamp })?;
    player::commit(playback.revisions, stamp.revision, &cmd);
    playback.transport.ab_loop = None;
    Ok(cmd)
}
