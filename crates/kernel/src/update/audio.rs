use std::sync::Arc;

use crate::{
    cmd::Cmd,
    domain::{
        cursor::Cursor,
        device::OutputDevice,
        direction::Direction,
        index::ViewIndex,
        player::Player,
        playlist::{Playlist, RepeatMode, index_of},
        revision::{Freshness, Revision},
        time::Moment,
        toast::Toast,
        track::{Track, TrackRef},
        transport::StreamError,
        workspace::Workspace,
    },
    message::{AudioError, AudioEvent},
    update::{
        machine::{Machine, Unhandled},
        player,
        player::{
            PlaybackParts,
            PlayerMessage,
            stamp::{Anchor, Stamp},
        },
        successor::{Successor, first_queued, successor},
        transport::TransportMessage,
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
            let recovered = playback
                .transport
                .transition(TransportMessage::OutputReady)?;
            Ok(cmd.then(recovered))
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
            let recovered = playback
                .transport
                .transition(TransportMessage::OutputReady)?;
            Ok(cmd.then(recovered))
        }
        AudioEvent::Error(failure) => error(playback, failure, now),
        AudioEvent::OutputLost(stream_error) => {
            output_lost(playback, stream_error, now)
        }
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
        crate::domain::device::DeviceName::to_string,
    );
    let told = format!("output device '{requested}' is gone — playing on {opened}");
    Ok(playback.workspace.show(
        Toast::error("Output device lost").with_text(told),
        playback.revisions,
    ))
}

fn error(
    playback: &mut PlaybackParts<'_>,
    error: AudioError,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let told = error.to_string();
    let stopped = player::record(playback, PlayerMessage::Error(error), now)?;
    let raised = playback.workspace.show(
        Toast::error("Audio error").with_text(told),
        playback.revisions,
    );
    Ok(raised.then(stopped))
}

fn output_lost(
    playback_parts: &mut PlaybackParts<'_>,
    stream_error: StreamError,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let told = output_lost_text(playback_parts.player, stream_error);
    let recorded = playback_parts
        .transport
        .transition(TransportMessage::OutputLost(stream_error))?;
    let stopped = player::record(playback_parts, PlayerMessage::OutputLost(now), now)?;
    let raised = playback_parts.workspace.show(
        Toast::error("Audio error").with_text(told),
        playback_parts.revisions,
    );
    Ok(raised.then(stopped).then(recorded))
}

fn output_lost_text(player: &Player, stream_error: StreamError) -> String {
    match player {
        Player::Playing { .. } | Player::Paused { .. } => {
            "Output lost — paused".to_string()
        }
        Player::Loading(..) | Player::Stopped => {
            format!("Audio output lost: {stream_error}")
        }
    }
}

fn cursor_to(playlist: &mut Playlist, dequeued: ViewIndex) {
    playlist.cursor = Cursor::at(playlist.tracks.len(), dequeued.get());
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
    let track = pop_queued_track(playback.playlist, playback.queue)
        .or_else(|| playback.playlist.skip(Direction::Next).cloned())
        .ok_or(Unhandled)?;
    let cmd = start(playback, track, now)?;
    follow_playback(playback.workspace, playback.playlist);
    Ok(cmd)
}

pub(crate) fn jump_to(
    playback: &mut PlaybackParts<'_>,
    index: ViewIndex,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let track = playback.playlist.jump(index).cloned().ok_or(Unhandled)?;
    start(playback, track, now)
}

pub(crate) fn previous(
    playback: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let track = playback
        .playlist
        .skip(Direction::Previous)
        .cloned()
        .ok_or(Unhandled)?;
    let cmd = start(playback, track, now)?;
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
        return Err(Unhandled);
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
    let reset = match pick.track() {
        Some(_) => match playback.transport.ab_loop {
            Some(..) => playback
                .transport
                .transition(TransportMessage::TrackChanged)?,
            None => Cmd::none(),
        },
        None => Cmd::none(),
    };
    move_onto(playback.playlist, playback.queue, pick);
    Ok(cmd.then(reset))
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
    let reset = match playback.transport.ab_loop {
        Some(..) => playback
            .transport
            .transition(TransportMessage::TrackChanged)?,
        None => Cmd::none(),
    };
    move_onto(playback.playlist, playback.queue, pick);
    if following {
        follow_playback(playback.workspace, playback.playlist);
    }
    Ok(cmd.then(reset))
}

fn was_following(workspace: &Workspace, playlist: &Playlist) -> bool {
    playlist.playing_index() == Some(workspace.browse.selected())
}

fn follow_playback(workspace: &mut Workspace, playlist: &Playlist) {
    if let Some(anchor) = playlist.playing_index() {
        workspace.browse.cursor = Cursor::at(playlist.tracks.len(), anchor.get());
    }
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
    let committed_index =
        index_of(&playlist.tracks, committed.source()).map(ViewIndex::new);
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
    let cmd =
        player::update_player(playback, PlayerMessage::Start { track, stamp }, now)?;
    let reset = match playback.transport.ab_loop {
        Some(..) => playback
            .transport
            .transition(TransportMessage::TrackChanged)?,
        None => Cmd::none(),
    };
    Ok(cmd.then(reset))
}
