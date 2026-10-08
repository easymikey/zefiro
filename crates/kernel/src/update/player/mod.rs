pub mod events;
mod requests;
pub mod scrobble;
pub mod stamp;

use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, RemoteCmd, TrackLoad},
    domain::{
        cue::PlaybackChange,
        player::{AbLoop, PausedBy, Player},
        playhead::Playhead,
        revision::Revision,
        server::{CacheKey, Download, MediaFetch, Server},
        time::Moment,
        toast::Toast,
        track::{Track, TrackSource},
    },
    message::{AudioError, Timer},
    update::{
        machine::{Machine, Unhandled},
        player::{
            events::{
                Lookahead,
                PlaybackParts,
                duration_of,
                handover_effects,
                next_decision,
                session,
            },
            scrobble::{now_playing, scrobble_timer},
            stamp::{Anchor, Stamp, StartOrigin},
        },
        successor::successor,
    },
};

#[derive(Debug)]
pub enum PlayerMessage {
    Toggle {
        current: Option<Arc<Track>>,
        stamp: Stamp,
    },
    Stop,
    Hold(Moment),
    Release(Anchor),
    Seek {
        target: Duration,
        now: Moment,
    },
    SleepFired(Moment),
    OutputLost(Moment),
    Loaded {
        duration: Option<Duration>,
        anchor: Anchor,
    },
    Error(AudioError),
    LookaheadReached {
        position: Duration,
        lookahead: Lookahead,
    },
    PositionReported {
        position: Duration,
        now: Moment,
    },
    TrackChanged {
        next: Option<Arc<Track>>,
        now: Moment,
    },
    Ended {
        next: Option<Arc<Track>>,
        stamp: Stamp,
    },
    SpeedChanged(Anchor),
}

impl Machine for Player {
    type Message = PlayerMessage;
    type Effect = Cmd;

    fn transition(&mut self, player_message: PlayerMessage) -> Result<Cmd, Unhandled> {
        match player_message {
            PlayerMessage::Toggle { current, stamp } => self.toggle(current, stamp),
            PlayerMessage::OutputLost(now) => self.output_lost(now),
            PlayerMessage::Stop => match self {
                Player::Loading(..)
                | Player::Playing { .. }
                | Player::Paused { .. } => Ok(self.stop()),
                Player::Stopped => Err(Unhandled),
            },
            PlayerMessage::Hold(now) => self.pause(now, PausedBy::Overlay),
            PlayerMessage::Release(anchor) => self.release(anchor),
            PlayerMessage::Seek { target, now } => self.seek(target, now),
            PlayerMessage::SleepFired(now) => self.pause(now, PausedBy::Listener),
            PlayerMessage::Loaded { duration, anchor } => self.loaded(duration, anchor),
            PlayerMessage::Error(error) => self.failed(&error),
            PlayerMessage::SpeedChanged(anchor) => match self {
                Player::Playing { playhead, .. } => {
                    *playhead = Playhead::anchored(
                        playhead.position_at(anchor.started_at),
                        anchor.started_at,
                        anchor.speed,
                    );
                    Ok(Cmd::none())
                }
                Player::Loading(..) | Player::Paused { .. } | Player::Stopped => {
                    Err(Unhandled)
                }
            },
            PlayerMessage::LookaheadReached {
                position,
                lookahead,
            } => self.positioned(position, lookahead),
            PlayerMessage::PositionReported { position, now } => {
                self.reported(position, now)
            }
            PlayerMessage::TrackChanged { next, now } => self.track_changed(next, now),
            PlayerMessage::Ended { next, stamp } => self.ended(next, stamp),
        }
    }
}

impl Player {
    fn stop(&mut self) -> Cmd {
        *self = Player::Stopped;
        PlaybackChange::Stop
            .cued()
            .then(Cmd::from(Effect::Macos(MacosCmd::NowPlaying(None))))
    }

    fn start(&mut self, track: Arc<Track>, origin: StartOrigin) -> Cmd {
        let track_load = TrackLoad::for_track(&track, origin.stamp().revision);
        let load_effect =
            track_load.map(|track_load| Effect::Audio(AudioCmd::Load(track_load)));
        let cmd = origin
            .stop()
            .into_iter()
            .chain(load_effect)
            .chain(handover_effects(
                &track,
                PlaybackChange::Play,
                origin.stamp().anchor.started_at,
            ))
            .collect();
        *self = Player::Loading(track);
        cmd
    }
}

pub(crate) fn update_player(
    playback_parts: &mut PlaybackParts<'_>,
    player_message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let seeks = matches!(player_message, PlayerMessage::Seek { .. });
    let track_changed = matches!(player_message, PlayerMessage::TrackChanged { .. });
    let candidate = playback_parts.revisions.effects.next();
    let paused = matches!(playback_parts.player, Player::Paused { .. });
    let (player_message, media_fetch, refusal) =
        match upcoming(playback_parts.player, &player_message)
            .map(|track| stream(playback_parts.servers, track, candidate))
            .transpose()
        {
            Ok(media_fetch) => (player_message, media_fetch.flatten(), Cmd::none()),
            Err(refusal) => {
                let PlayerMessage::Ended { stamp, .. } = player_message else {
                    return Ok(refusal);
                };
                (PlayerMessage::Ended { next: None, stamp }, None, refusal)
            }
        };
    let cmd = playback_parts.player.transition(player_message)?;
    playback_parts.revisions.effects = candidate;
    let cmd = cmd.then(fetch(playback_parts, media_fetch, now));
    let cmd = if track_changed {
        cmd.then(now_playing(playback_parts, now))
    } else {
        cmd
    };
    let armed = if seeks {
        timer(playback_parts, now)
    } else {
        arm(playback_parts, now)
    };
    let resumed = if paused
        && matches!(playback_parts.player, Player::Playing { .. })
        && playback_parts.revisions.scrobble.is_some()
    {
        scrobble_timer(playback_parts, candidate, now)
    } else {
        Cmd::none()
    };
    Ok(cmd.then(armed).then(resumed).then(refusal))
}

fn upcoming<'a>(
    player: &Player,
    player_message: &'a PlayerMessage,
) -> Option<&'a Arc<Track>> {
    match player_message {
        PlayerMessage::Toggle { current, .. } => current
            .as_ref()
            .filter(|_track| matches!(player, Player::Stopped)),
        PlayerMessage::Ended { next, .. } => next
            .as_ref()
            .filter(|_track| matches!(player, Player::Playing { .. })),
        PlayerMessage::LookaheadReached {
            position,
            lookahead,
        } => lookahead.next.as_ref().filter(|_track| {
            lookahead.is_preload_due(*position)
                && matches!(
                    player,
                    Player::Playing {
                        preloaded: None,
                        ..
                    }
                )
        }),
        PlayerMessage::Stop
        | PlayerMessage::Hold(_)
        | PlayerMessage::Release(_)
        | PlayerMessage::Seek { .. }
        | PlayerMessage::SleepFired(_)
        | PlayerMessage::OutputLost(_)
        | PlayerMessage::Loaded { .. }
        | PlayerMessage::Error(_)
        | PlayerMessage::PositionReported { .. }
        | PlayerMessage::TrackChanged { .. }
        | PlayerMessage::SpeedChanged(_) => None,
    }
}

pub(crate) fn start(
    playback_parts: &mut PlaybackParts<'_>,
    track: Arc<Track>,
    now: Moment,
) -> Result<Cmd, Cmd> {
    let stamp = Stamp::pending(playback_parts.transport, playback_parts.revisions, now);
    let media_fetch = stream(playback_parts.servers, &track, stamp.revision)?;
    let started = playback_parts.player.start(track, StartOrigin::User(stamp));
    playback_parts.revisions.effects = stamp.revision;
    playback_parts.transport.track_changed();
    Ok(started.then(fetch(playback_parts, media_fetch, now)))
}

fn stream(
    servers: &[Server],
    track: &Track,
    revision: Revision,
) -> Result<Option<MediaFetch>, Cmd> {
    let TrackSource::Server {
        server_name,
        server_track_id,
    } = track.source()
    else {
        return Ok(None);
    };
    let session = session(servers, server_name).ok_or_else(|| {
        Cmd::message(crate::message::Message::Toast(Toast::error(format!(
            "{server_name} is offline"
        ))))
    })?;
    Ok(Some(MediaFetch {
        server_name: server_name.clone(),
        server_track_id: server_track_id.clone(),
        cache_key: CacheKey::new(
            server_name,
            server_track_id,
            track.audio_format().format.as_deref().unwrap_or(""),
        ),
        session: session.clone(),
        first_byte: 0,
        revision,
    }))
}

fn fetch(
    playback_parts: &mut PlaybackParts<'_>,
    media_fetch: Option<MediaFetch>,
    now: Moment,
) -> Cmd {
    let player = &*playback_parts.player;
    playback_parts.downloads.retain(|download| {
        player
            .current()
            .into_iter()
            .chain(player.preloaded())
            .any(|track| track.holds(&download.media_fetch))
            && media_fetch.as_ref().is_none_or(|media_fetch| {
                download.media_fetch.server_name != media_fetch.server_name
                    || download.media_fetch.server_track_id
                        != media_fetch.server_track_id
            })
    });
    let Some(media_fetch) = media_fetch else {
        return Cmd::none();
    };
    let download = Download {
        media_fetch,
        fetched: None,
    };
    let chunk = chunk(&download, player);
    playback_parts.downloads.push(download);
    let reported = if matches!(chunk, Some(RemoteCmd::Fetch(_))) {
        now_playing(playback_parts, now)
    } else {
        Cmd::none()
    };
    reported.then(Cmd::from_iter(chunk.map(Effect::Remote)))
}

pub(crate) fn lookahead(playback_parts: &PlaybackParts<'_>, now: Moment) -> Lookahead {
    let ab_loop = match playback_parts.transport.ab_loop {
        Some(AbLoop::BothMarked {
            loop_start,
            loop_end,
        }) => Some((loop_start, loop_end)),
        Some(AbLoop::StartMarked(_)) | None => None,
    };
    Lookahead {
        ab_loop,
        next: playback_parts
            .player
            .preloaded()
            .is_none()
            .then(|| {
                successor(playback_parts.playlist, playback_parts.queue).into_track()
            })
            .flatten()
            .filter(|track| match track.source() {
                TrackSource::Local(_path) => true,
                TrackSource::Server { server_name, .. } => {
                    session(playback_parts.servers, server_name).is_some()
                        && playback_parts
                            .player
                            .current()
                            .is_none_or(|current| current.source() != track.source())
                }
            }),
        duration: duration_of(playback_parts.player),
        now,
        revision: playback_parts.revisions.effects.next(),
        cover_side: crate::update::cover_side(
            playback_parts.workspace,
            playback_parts.settings,
        ),
    }
}

pub(crate) fn arm(playback_parts: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { playhead, .. } = &*playback_parts.player else {
        return Cmd::none();
    };
    Cmd::from(Effect::Macos(MacosCmd::SetPosition(
        playhead.position_at(now),
    )))
    .then(timer(playback_parts, now))
}

fn timer(playback_parts: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Player::Playing { playhead, .. } = &*playback_parts.player else {
        return Cmd::none();
    };
    next_decision(*playhead, &lookahead(playback_parts, now)).map_or(
        Cmd::none(),
        |delay| {
            Effect::After {
                delay,
                timer: Timer::Lookahead(playback_parts.revisions.issue_lookahead()),
            }
            .into()
        },
    )
}

pub(crate) fn stopped_effects() -> Cmd {
    PlaybackChange::Stop
        .effects()
        .into_iter()
        .chain([Effect::Macos(MacosCmd::NowPlaying(None))])
        .collect()
}

pub(crate) fn retry(
    downloads: &[Download],
    player: &Player,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    downloads
        .iter()
        .find(|download| download.media_fetch.revision == revision)
        .and_then(|download| chunk(download, player))
        .map(|remote_cmd| Cmd::from(Effect::Remote(remote_cmd)))
        .ok_or(Unhandled)
}

pub(crate) fn chunk(download: &Download, player: &Player) -> Option<RemoteCmd> {
    let media_fetch = &download.media_fetch;
    let first_byte = match &download.fetched {
        None => media_fetch.first_byte,
        Some(fetched) if fetched.is_complete() => return None,
        Some(fetched) => fetched.downloaded,
    };
    let next_media_fetch = MediaFetch {
        server_name: media_fetch.server_name.clone(),
        server_track_id: media_fetch.server_track_id.clone(),
        cache_key: media_fetch.cache_key.clone(),
        session: media_fetch.session.clone(),
        first_byte,
        revision: media_fetch.revision,
    };
    let current = player
        .current()
        .is_some_and(|track| track.holds(media_fetch));
    Some(if current {
        RemoteCmd::Fetch(next_media_fetch)
    } else {
        RemoteCmd::Prefetch(next_media_fetch)
    })
}

pub(crate) fn preload(download: &Download, player: &Player) -> Option<AudioCmd> {
    player
        .preloaded()
        .filter(|track| track.holds(&download.media_fetch))
        .and_then(|track| TrackLoad::fetched(track, download))
        .map(AudioCmd::Preload)
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, DiskCmd, Effect, LibraryCmd, MacosCmd, TrackLoad},
        domain::{
            cue::{Cue, PlaybackChange},
            history::HistoryEntry,
            model::Model,
            player::{PausedBy, Player},
            playhead::Playhead,
            revision::Revision,
            server::{
                Account,
                Endpoint,
                Fetched,
                Server,
                ServerName,
                ServerStatus,
                ServerTrackId,
                Session,
                UserName,
            },
            speed::Speed,
            time::Moment,
            track::{Track, TrackSource},
        },
        message::{Message, RemoteEvent, Timer},
        update::{machine::Unhandled, playback_parts, player::start, update},
    };

    const AT: Duration = Duration::from_secs(5);

    fn now() -> Moment {
        Moment::new(Duration::from_secs(1_000))
    }

    fn track_a() -> Arc<Track> {
        Arc::new(Track::listed(Path::new("/tmp/a.flac")))
    }

    fn track_b() -> Arc<Track> {
        Arc::new(Track::listed(Path::new("/tmp/b.flac")))
    }

    fn playing(track: Arc<Track>, preloaded: Option<Arc<Track>>) -> Player {
        Player::Playing {
            track,
            playhead: Playhead::anchored(AT, now(), Speed::default()),
            preloaded,
        }
    }

    fn paused(track: Arc<Track>) -> Player {
        Player::Paused {
            track,
            position: AT,
            by: PausedBy::Listener,
        }
    }

    fn cut_in(track: &Arc<Track>) -> Cmd {
        let mut effects = vec![
            Effect::Audio(AudioCmd::Stop),
            Effect::Audio(AudioCmd::Load(
                TrackLoad::for_track(track, Revision::default().next()).unwrap(),
            )),
            Effect::Library(LibraryCmd::Disk(DiskCmd::AppendHistory(
                HistoryEntry::from_track(track, now()),
            ))),
            Effect::Macos(MacosCmd::NowPlaying(Some(Arc::clone(track)))),
        ];
        effects.extend(PlaybackChange::Play.effects());
        effects.push(Effect::Animate(Cue::TrackChanged));
        effects.push(Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Play)));
        Cmd::from_iter(effects)
    }

    #[rstest]
    #[case::stopped_next_starts(Player::Stopped)]
    #[case::loading_next_interrupts_the_load(Player::Loading(track_a()))]
    #[case::playing_next_drops_the_preload_and_starts(playing(
        track_a(),
        Some(track_a())
    ))]
    #[case::paused_next_starts(paused(track_a()))]
    fn a_start_loads_the_track_in_every_state(#[case] player: Player) {
        let mut model = Model {
            player,
            ..Model::default()
        };

        let cmd = start(&mut playback_parts(&mut model), track_b(), now());

        assert_eq!(model.player, Player::Loading(track_b()));
        assert_eq!(cmd, Ok(cut_in(&track_b())));
    }

    fn session() -> Session {
        Session::new(
            Endpoint::parse("https://music.example.com").unwrap(),
            "u=ann&t=token&s=salt",
        )
    }

    fn online(player: Player) -> Model {
        Model {
            player,
            servers: vec![Server {
                account: Account {
                    server_name: ServerName::new("home"),
                    endpoint: Endpoint::parse("https://music.example.com").unwrap(),
                    user_name: UserName::new("ann").unwrap(),
                },
                server_status: ServerStatus::Online(session()),
            }],
            ..Model::default()
        }
    }

    #[test]
    fn playing_another_track_drops_the_unfinished_download_of_the_last() {
        let track = |id: &str| {
            Arc::new(Track::from(TrackSource::Server {
                server_name: ServerName::new("home"),
                server_track_id: ServerTrackId::new(id),
            }))
        };
        let first = Revision::default().next();
        let mut model = online(Player::Stopped);
        assert!(start(&mut playback_parts(&mut model), track("tr-1"), now()).is_ok());

        assert!(start(&mut playback_parts(&mut model), track("tr-2"), now()).is_ok());

        assert_eq!(
            model
                .downloads
                .iter()
                .map(|download| download.media_fetch.revision)
                .collect::<Vec<_>>(),
            vec![first.next()]
        );
        assert_eq!(
            update(&mut model, Message::Elapsed(Timer::Fetch(first)), now()),
            Err(Unhandled)
        );
        let downloads = model.downloads.clone();
        let remote_event = RemoteEvent::Fetched {
            revision: first,
            result: Ok(Fetched {
                media_path: PathBuf::from("/cache/home/tr-1.flac.part"),
                downloaded: 1024,
                byte_len: 4096,
            }),
        };
        assert_eq!(
            update(&mut model, Message::Remote(remote_event), now()),
            Err(Unhandled)
        );
        assert_eq!(model.downloads, downloads);
    }
}
