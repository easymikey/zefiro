pub mod events;
mod requests;
pub mod stamp;

use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, TrackLoad},
    domain::{
        cue::PlaybackChange,
        player::{AbLoop, PausedBy, Player},
        playhead::Playhead,
        playlist::Playlist,
        revision::Revisions,
        settings::Settings,
        time::Moment,
        track::{Track, TrackSource},
        transport::Transport,
        workspace::Workspace,
    },
    message::{AudioError, Timer},
    update::{
        machine::{Machine, Unhandled},
        player::{
            events::{Lookahead, handover_effects, next_decision},
            stamp::{Anchor, Stamp, StartOrigin},
        },
        successor::successor,
    },
};

pub(crate) struct PlaybackParts<'a> {
    pub(crate) player: &'a mut Player,
    pub(crate) transport: &'a mut Transport,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) queue: &'a mut Vec<TrackSource>,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) settings: &'a mut Settings,
}

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

    fn transition(&mut self, message: PlayerMessage) -> Result<Cmd, Unhandled> {
        match message {
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
            PlayerMessage::SleepFired(now) => match self {
                Player::Playing { .. } => self.pause(now, PausedBy::Listener),
                Player::Loading(..) | Player::Paused { .. } | Player::Stopped => {
                    Err(Unhandled)
                }
            },
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
        let load_effect = Effect::Audio(AudioCmd::Load(track_load));
        let cmd = origin
            .stop()
            .into_iter()
            .chain([load_effect])
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
    message: PlayerMessage,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let seeks = matches!(message, PlayerMessage::Seek { .. });
    let candidate = playback_parts.revisions.effects.next();
    let cmd = playback_parts.player.transition(message)?;
    playback_parts.revisions.effects = candidate;
    let armed = if seeks {
        timer(playback_parts, now)
    } else {
        arm(playback_parts, now)
    };
    Ok(cmd.then(armed))
}

pub(crate) fn start(
    playback_parts: &mut PlaybackParts<'_>,
    track: Arc<Track>,
    now: Moment,
) -> Cmd {
    let stamp = Stamp::pending(playback_parts.transport, playback_parts.revisions, now);
    let started = playback_parts.player.start(track, StartOrigin::User(stamp));
    playback_parts.revisions.effects = stamp.revision;
    playback_parts.transport.track_changed();
    started
}

pub(crate) fn duration_of(player: &Player) -> Duration {
    player
        .current()
        .and_then(|track| track.duration())
        .unwrap_or(Duration::ZERO)
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
        next: successor(playback_parts.playlist, playback_parts.queue)
            .track()
            .cloned(),
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

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

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
            speed::Speed,
            time::Moment,
            track::Track,
        },
        update::{playback_parts, player::start},
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
            Effect::Audio(AudioCmd::Load(TrackLoad::for_track(
                track,
                Revision::default().next(),
            ))),
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
        assert_eq!(cmd, cut_in(&track_b()));
    }
}
