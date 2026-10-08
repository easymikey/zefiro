use std::sync::Arc;

use crate::{
    cmd::Cmd,
    domain::{
        cursor::Cursor,
        device::{DeviceName, OutputDevice},
        direction::Direction,
        index::ViewIndex,
        player::Player,
        playlist::{Playlist, RepeatMode, index_of},
        revision::{Freshness, Revision},
        time::Moment,
        toast::Toast,
        track::{Track, TrackSource},
        transport::{OutputError, OutputStatus},
        workspace::Workspace,
    },
    message::{AudioError, AudioEvent},
    update::{
        machine::{Unhandled, replace},
        player,
        player::{
            PlayerMessage,
            events::PlaybackParts,
            stamp::{Anchor, Stamp},
        },
        successor::{Successor, first_queued, successor},
    },
};

pub(crate) fn update(
    playback_parts: &mut PlaybackParts<'_>,
    event: AudioEvent,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match event {
        AudioEvent::PositionReported(position) => {
            let cmd = player::update_player(
                playback_parts,
                PlayerMessage::PositionReported { position, now },
                now,
            )?;
            playback_parts.transport.output_ready();
            Ok(cmd)
        }
        AudioEvent::TrackChanged => track_changed(playback_parts, now),
        AudioEvent::Ended => {
            let following =
                was_following(playback_parts.workspace, playback_parts.playlist);
            let cmd = ended(playback_parts, now)?;
            if following {
                follow_playback(playback_parts.workspace, playback_parts.playlist);
            }
            Ok(cmd)
        }
        AudioEvent::Loaded(duration) => {
            let anchor = Anchor::at(playback_parts.transport, now);
            let cmd = player::update_player(
                playback_parts,
                PlayerMessage::Loaded { duration, anchor },
                now,
            )?;
            playback_parts.transport.output_ready();
            playback_parts.transport.buffering_revision = None;
            Ok(cmd)
        }
        AudioEvent::Error(error) => self::error(playback_parts, error, now),
        AudioEvent::OutputLost(error) => output_lost(playback_parts, error, now),
        AudioEvent::DevicesListed(devices) => {
            replace(&mut playback_parts.settings.output_devices, devices)
                .map(|()| Cmd::none())
        }
        AudioEvent::DeviceFellBack(output_device) => {
            fell_back(playback_parts, &output_device)
        }
        AudioEvent::DeviceOpened(device_name) => opened(playback_parts, device_name),
        AudioEvent::Buffering(revision) => replace(
            &mut playback_parts.transport.buffering_revision,
            Some(revision),
        )
        .map(|()| Cmd::none()),
        AudioEvent::Buffered(revision) => {
            if playback_parts.transport.buffering_revision != Some(revision) {
                return Err(Unhandled);
            }
            playback_parts.transport.buffering_revision = None;
            Ok(Cmd::none())
        }
        AudioEvent::PreloadCancelled(revision) => {
            preload_cancelled(playback_parts, revision)
        }
        AudioEvent::PreloadKept(revision) => preloaded(playback_parts, revision)
            .map(|_track| Cmd::none())
            .ok_or(Unhandled),
    }
}

fn preloaded<'a>(
    playback_parts: &'a PlaybackParts<'_>,
    revision: Revision,
) -> Option<&'a Arc<Track>> {
    playback_parts.player.preloaded().filter(|track| {
        playback_parts.downloads.iter().any(|download| {
            download.media_fetch.revision == revision
                && track.holds(&download.media_fetch)
        })
    })
}

fn preload_cancelled(
    playback_parts: &mut PlaybackParts<'_>,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    if preloaded(playback_parts, revision).is_none() {
        return Err(Unhandled);
    }
    let Player::Playing {
        track,
        preloaded: preloaded @ Some(_),
        ..
    } = &mut *playback_parts.player
    else {
        return Err(Unhandled);
    };
    *preloaded = None;
    playback_parts
        .downloads
        .retain(|download| track.holds(&download.media_fetch));
    Ok(Cmd::none())
}

fn fell_back(
    playback_parts: &mut PlaybackParts<'_>,
    output_device: &OutputDevice,
) -> Result<Cmd, Unhandled> {
    if *output_device == playback_parts.settings.audio_settings.device {
        return Err(Unhandled);
    }
    let requested = std::mem::replace(
        &mut playback_parts.settings.audio_settings.device,
        output_device.clone(),
    );
    playback_parts.settings.device_name = None;
    let OutputDevice::Named(requested) = requested else {
        return Ok(Cmd::none());
    };
    let fallback_device_text = output_device
        .named()
        .map_or_else(|| "the system default".to_string(), DeviceName::to_string);
    let toast_text = format!(
        "output device '{requested}' is gone — playing on {fallback_device_text}"
    );
    Ok(playback_parts.workspace.show(
        Toast::error("Output device lost").with_text(toast_text),
        playback_parts.revisions,
    ))
}

fn opened(
    playback_parts: &mut PlaybackParts<'_>,
    device_name: DeviceName,
) -> Result<Cmd, Unhandled> {
    let toast = Toast::info(format!("Playing on {device_name}"));
    let first = playback_parts.settings.device_name.is_none();
    replace(&mut playback_parts.settings.device_name, Some(device_name))?;
    if first {
        return Ok(Cmd::none());
    }
    Ok(playback_parts
        .workspace
        .show(toast, playback_parts.revisions))
}

fn error(
    playback_parts: &mut PlaybackParts<'_>,
    error: AudioError,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let toast_text = error.to_string();
    let stopped =
        player::update_player(playback_parts, PlayerMessage::Error(error), now);
    let raised = playback_parts.workspace.show(
        Toast::error("Audio error").with_text(toast_text),
        playback_parts.revisions,
    );
    match stopped {
        Ok(stopped) => Ok(raised.then(stopped)),
        Err(Unhandled) => Ok(raised),
    }
}

fn output_lost(
    playback_parts: &mut PlaybackParts<'_>,
    error: OutputError,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let toast_text = output_lost_text(playback_parts.player, error);
    let recorded = replace(
        &mut playback_parts.transport.output_status,
        OutputStatus::Lost(error),
    );
    let stopped = match player::update_player(
        playback_parts,
        PlayerMessage::OutputLost(now),
        now,
    ) {
        Ok(stopped) => stopped,
        Err(Unhandled) => recorded.map(|()| Cmd::none())?,
    };
    Ok(playback_parts
        .workspace
        .show(
            Toast::error("Audio error").with_text(toast_text),
            playback_parts.revisions,
        )
        .then(stopped))
}

fn output_lost_text(player: &Player, error: OutputError) -> String {
    match player {
        Player::Playing { .. } | Player::Paused { .. } => {
            "Output lost — paused".to_string()
        }
        Player::Loading(..) | Player::Stopped => {
            format!("Audio output lost: {error}")
        }
    }
}

pub(crate) fn next(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let pick = first_queued(playback_parts.playlist, playback_parts.queue).map_or_else(
        || {
            playback_parts
                .playlist
                .upcoming()
                .cloned()
                .map_or(Successor::Nothing, Successor::Following)
        },
        |(queue_index, index, track)| Successor::Queued {
            queue_index,
            index,
            track: Arc::clone(track),
        },
    );
    let track = pick.track().cloned().ok_or(Unhandled)?;
    Ok(match player::start(playback_parts, track, now) {
        Ok(cmd) => {
            move_onto(playback_parts.playlist, playback_parts.queue, pick);
            follow_playback(playback_parts.workspace, playback_parts.playlist);
            cmd
        }
        Err(refusal) => refusal,
    })
}

pub(crate) fn jump_to(
    playback_parts: &mut PlaybackParts<'_>,
    index: ViewIndex,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    Ok(match jump(playback_parts, index, now)? {
        Ok(cmd) | Err(cmd) => cmd,
    })
}

pub(crate) fn previous(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let index = playback_parts
        .playlist
        .next_index(Direction::Previous)
        .map(ViewIndex::new)
        .ok_or(Unhandled)?;
    Ok(match jump(playback_parts, index, now)? {
        Ok(cmd) => {
            follow_playback(playback_parts.workspace, playback_parts.playlist);
            cmd
        }
        Err(refusal) => refusal,
    })
}

fn jump(
    playback_parts: &mut PlaybackParts<'_>,
    index: ViewIndex,
    now: Moment,
) -> Result<Result<Cmd, Cmd>, Unhandled> {
    let track = playback_parts
        .playlist
        .tracks
        .get(index.get())
        .cloned()
        .ok_or(Unhandled)?;
    Ok(player::start(playback_parts, track, now)
        .inspect(|_cmd| playback_parts.playlist.point_at(index)))
}

pub(crate) fn lookahead_fired(
    playback_parts: &mut PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    if matches!(
        revision.freshness(playback_parts.revisions.lookahead),
        Freshness::Stale
    ) || !playback_parts.player.is_playing()
    {
        return Err(Unhandled);
    }
    let position = playback_parts.player.position_at(now);
    let lookahead = player::lookahead(playback_parts, now);
    let message = lookahead.loop_start(position).map_or_else(
        || PlayerMessage::LookaheadReached {
            position,
            lookahead,
        },
        |target| PlayerMessage::Seek { target, now },
    );
    player::update_player(playback_parts, message, now)
}

fn ended(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let stamp = Stamp::pending(playback_parts.transport, playback_parts.revisions, now);
    let pick = successor(playback_parts.playlist, playback_parts.queue);
    let message = PlayerMessage::Ended {
        next: pick.track().cloned(),
        stamp,
    };
    let cmd = player::update_player(playback_parts, message, now)?;
    if playback_parts.player.current().is_some() {
        playback_parts.transport.track_changed();
        move_onto(playback_parts.playlist, playback_parts.queue, pick);
    }
    Ok(cmd)
}

fn track_changed(
    playback_parts: &mut PlaybackParts<'_>,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let following = was_following(playback_parts.workspace, playback_parts.playlist);
    let pick = match playback_parts.player.preloaded() {
        Some(committed) => Successor::Preloaded(Arc::clone(committed)),
        None if matches!(playback_parts.playlist.repeat_mode, RepeatMode::One) => {
            Successor::Nothing
        }
        None => successor(playback_parts.playlist, playback_parts.queue),
    };
    let message = PlayerMessage::TrackChanged {
        next: pick.track().cloned(),
        now,
    };
    let cmd = player::update_player(playback_parts, message, now)?;
    playback_parts.transport.track_changed();
    move_onto(playback_parts.playlist, playback_parts.queue, pick);
    if following {
        follow_playback(playback_parts.workspace, playback_parts.playlist);
    }
    Ok(cmd)
}

fn was_following(workspace: &Workspace, playlist: &Playlist) -> bool {
    playlist.playing_index() == Some(workspace.browse.selected())
}

fn follow_playback(workspace: &mut Workspace, playlist: &Playlist) {
    if let Some(anchor) = playlist.playing_index() {
        workspace.browse.cursor = Cursor::at(playlist.tracks.len(), anchor.get());
    }
}

fn move_onto(
    playlist: &mut Playlist,
    queue: &mut Vec<TrackSource>,
    successor: Successor,
) {
    match successor {
        Successor::Preloaded(committed) => {
            move_onto_preloaded(playlist, queue, &committed);
        }
        Successor::Queued {
            queue_index, index, ..
        } => {
            queue.drain(..=queue_index);
            playlist.point_at(index);
        }
        Successor::Following(_) => {
            playlist.skip(Direction::Next);
        }
        Successor::Repeating(_) | Successor::Nothing => {}
    }
}

fn move_onto_preloaded(
    playlist: &mut Playlist,
    queue: &mut Vec<TrackSource>,
    committed_track: &Arc<Track>,
) {
    let committed_index =
        index_of(&playlist.tracks, committed_track.source()).map(ViewIndex::new);
    let Some(committed_index) = committed_index else {
        return;
    };
    queue.retain(|queued| queued != committed_track.source());
    playlist.point_at(committed_index);
}
