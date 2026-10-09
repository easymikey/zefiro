use std::{mem, sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, CoverJob, DiskCmd, Effect, LibraryCmd, MacosCmd, TrackLoad},
    domain::{
        cue::{Cue, PlaybackChange},
        geometry::Pixels,
        history::HistoryEntry,
        player::{PausedBy, Player},
        playhead::Playhead,
        playlist::Playlist,
        revision::{Revision, Revisions},
        server::{Download, Server, ServerName, ServerStatus, Session},
        settings::Settings,
        time::Moment,
        track::Track,
        transport::{PRELOAD_LEAD, Transport},
        workspace::Workspace,
    },
    message::AudioError,
    update::{
        machine::Unhandled,
        player::stamp::{Anchor, Stamp, StartOrigin},
    },
};

pub(crate) struct PlaybackParts<'a> {
    pub(crate) player: &'a mut Player,
    pub(crate) transport: &'a mut Transport,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) queue: &'a mut Vec<Arc<Track>>,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) settings: &'a mut Settings,
    pub(crate) servers: &'a [Server],
    pub(crate) downloads: &'a mut Vec<Download>,
}

#[derive(Debug)]
pub struct Lookahead {
    pub ab_loop: Option<(Duration, Duration)>,
    pub next: Option<Arc<Track>>,
    pub duration: Duration,
    pub now: Moment,
    pub revision: Revision,
    pub cover_side: Option<Pixels>,
}

impl Lookahead {
    pub(crate) fn loop_start(&self, position: Duration) -> Option<Duration> {
        self.ab_loop.and_then(|(a, b)| (position >= b).then_some(a))
    }

    fn preload_due_at(&self) -> Option<Duration> {
        (self.next.is_some() && !self.duration.is_zero())
            .then(|| self.duration.saturating_sub(PRELOAD_LEAD))
    }

    pub(crate) fn is_preload_due(&self, position: Duration) -> bool {
        self.preload_due_at().is_some_and(|due| position >= due)
    }

    fn preloading(self, position: Duration, preloaded: &mut Option<Arc<Track>>) -> Cmd {
        if !self.is_preload_due(position) {
            return Cmd::none();
        }
        let Some(next) = self.next else {
            return Cmd::none();
        };
        let preload_cmd_effect = TrackLoad::for_track(&next, self.revision)
            .map(|track_load| Effect::Audio(AudioCmd::Preload(track_load)));
        let prefetch = self.cover_side.zip(next.local_path()).map(|(side, path)| {
            Effect::Library(LibraryCmd::PrefetchCover(CoverJob {
                path: path.to_path_buf(),
                side,
            }))
        });
        *preloaded = Some(next);
        Cmd::from_iter(preload_cmd_effect.into_iter().chain(prefetch))
    }
}

impl Player {
    pub(crate) fn loaded(
        &mut self,
        duration: Option<Duration>,
        anchor: Anchor,
    ) -> Result<Cmd, Unhandled> {
        match mem::replace(self, Player::Stopped) {
            Player::Loading(track) => {
                let track = match duration {
                    Some(duration) => Arc::new(track.with_duration(duration)),
                    None => track,
                };
                *self = Player::Playing {
                    track,
                    playhead: Playhead::anchored(
                        Duration::ZERO,
                        anchor.started_at,
                        anchor.speed,
                    ),
                    preloaded: None,
                };
                Ok(Cmd::none())
            }
            other @ (Player::Playing { .. }
            | Player::Paused { .. }
            | Player::Stopped) => {
                *self = other;
                Err(Unhandled)
            }
        }
    }

    pub(crate) fn failed(&mut self, error: &AudioError) -> Result<Cmd, Unhandled> {
        match error {
            AudioError::Decode { .. } | AudioError::OpenDevice { .. } => {
                self.load_failed()
            }
            AudioError::ListDevices { .. }
            | AudioError::Preload { .. }
            | AudioError::Seek { .. } => Err(Unhandled),
        }
    }

    pub(crate) fn output_lost(&mut self, now: Moment) -> Result<Cmd, Unhandled> {
        match self {
            Player::Playing { .. } => self.pause(now, PausedBy::Listener),
            Player::Loading(..) => Ok(self.stop()),
            Player::Paused { .. } | Player::Stopped => Err(Unhandled),
        }
    }

    fn load_failed(&mut self) -> Result<Cmd, Unhandled> {
        match self {
            Player::Loading(..) => Ok(self.stop()),
            Player::Playing { .. } | Player::Paused { .. } | Player::Stopped => {
                Err(Unhandled)
            }
        }
    }

    pub(crate) fn positioned(
        &mut self,
        position: Duration,
        lookahead: Lookahead,
    ) -> Result<Cmd, Unhandled> {
        let reported = self.reported(position, lookahead.now)?;
        Ok(reported.then(match self {
            Player::Playing {
                preloaded: preloaded @ None,
                ..
            } => lookahead.preloading(position, preloaded),
            Player::Playing {
                preloaded: Some(_), ..
            }
            | Player::Paused { .. }
            | Player::Loading(..)
            | Player::Stopped => Cmd::none(),
        }))
    }

    pub(crate) fn reported(
        &mut self,
        position: Duration,
        now: Moment,
    ) -> Result<Cmd, Unhandled> {
        match self {
            Player::Playing { playhead, .. } => {
                *playhead = Playhead::anchored(position, now, playhead.speed);
                Ok(Cmd::none())
            }
            Player::Paused { .. } | Player::Loading(..) | Player::Stopped => {
                Err(Unhandled)
            }
        }
    }

    pub(crate) fn track_changed(
        &mut self,
        next: Option<Arc<Track>>,
        now: Moment,
    ) -> Result<Cmd, Unhandled> {
        let (track, change) = match self {
            Player::Playing {
                track,
                playhead,
                preloaded,
            } => {
                *playhead = Playhead::anchored(Duration::ZERO, now, playhead.speed);
                *preloaded = None;
                (track, PlaybackChange::Play)
            }
            Player::Paused {
                track, position, ..
            } => {
                *position = Duration::ZERO;
                (track, PlaybackChange::Pause)
            }
            Player::Loading(..) | Player::Stopped => return Err(Unhandled),
        };
        if let Some(next) = next {
            *track = next;
        }
        Ok(Cmd::from_iter(handover_effects(track, change, now)))
    }

    pub(crate) fn ended(
        &mut self,
        next: Option<Arc<Track>>,
        stamp: Stamp,
    ) -> Result<Cmd, Unhandled> {
        match self {
            Player::Playing { .. } => Ok(match next {
                Some(track) => self.start(track, StartOrigin::TrackEnded(stamp)),
                None => self.stop(),
            }),
            Player::Paused { .. } | Player::Loading(..) | Player::Stopped => {
                Err(Unhandled)
            }
        }
    }
}

pub(crate) fn session<'a>(
    servers: &'a [Server],
    server_name: &ServerName,
) -> Option<&'a Session> {
    servers
        .iter()
        .find(|server| server.account.server_name == *server_name)
        .and_then(|server| match &server.server_status {
            ServerStatus::Online(session) => Some(session),
            ServerStatus::Connecting | ServerStatus::Offline(_) => None,
        })
}

pub(crate) fn duration_of(player: &Player) -> Duration {
    player
        .current()
        .and_then(|track| track.duration())
        .unwrap_or(Duration::ZERO)
}

#[must_use]
pub(crate) fn next_decision(
    playhead: Playhead,
    lookahead: &Lookahead,
) -> Option<Duration> {
    let current = playhead.position_at(lookahead.now);
    let target = [
        lookahead.preload_due_at(),
        lookahead.ab_loop.map(|(_, b)| b),
    ]
    .into_iter()
    .flatten()
    .filter(|&point| point > current)
    .min()?;
    Some((target - current).div_f32(playhead.speed.get()))
}

pub(crate) fn seek_effect(target: Duration, revision: Revision) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Seek { target, revision }),
        Effect::Macos(MacosCmd::SetPosition(target)),
    ])
}

pub(crate) fn handover_effects(
    track: &Arc<Track>,
    playback_change: PlaybackChange,
    now: Moment,
) -> Vec<Effect> {
    let append_history = track.local_path().is_some().then(|| {
        Effect::Library(LibraryCmd::Disk(DiskCmd::AppendHistory(
            HistoryEntry::from_track(track, now),
        )))
    });
    append_history
        .into_iter()
        .chain([Effect::Macos(MacosCmd::NowPlaying(Some(Arc::clone(track))))])
        .chain(playback_change.effects())
        .chain([
            Effect::Animate(Cue::TrackChanged),
            Effect::Animate(Cue::PlaybackChanged(playback_change)),
        ])
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{DiskCmd, Effect, LibraryCmd},
        domain::{
            bounded::Bounded,
            cue::PlaybackChange,
            playhead::Playhead,
            revision::Revision,
            server::{ServerName, ServerTrackId},
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track, TrackParts, TrackSource},
        },
        update::player::events::{Lookahead, handover_effects, next_decision},
    };

    fn appends_history(track: &Arc<Track>) -> bool {
        handover_effects(track, PlaybackChange::Play, Moment::default())
            .iter()
            .any(|effect| {
                matches!(
                    effect,
                    Effect::Library(LibraryCmd::Disk(DiskCmd::AppendHistory(_)))
                )
            })
    }

    #[test]
    fn handover_effects_of_a_server_track_append_no_history() {
        let server_track = Arc::new(Track::from(TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new("tr-1"),
        }));

        assert!(appends_history(&a_track()));
        assert!(!appends_history(&server_track));
    }

    fn head_at(offset: u64, speed_factor: f32) -> Playhead {
        Playhead::anchored(
            Duration::from_secs(offset),
            Moment::new(Duration::ZERO),
            Speed::clamped(speed_factor),
        )
    }

    fn a_track() -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: "/tmp/next.flac".into(),
            duration: Duration::from_secs(1),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        }))
    }

    struct Setup {
        ab_loop: Option<(u64, u64)>,
        next: Option<Arc<Track>>,
        duration_secs: u64,
    }

    fn lookahead(setup: Setup) -> Lookahead {
        Lookahead {
            ab_loop: setup
                .ab_loop
                .map(|(a, b)| (Duration::from_secs(a), Duration::from_secs(b))),
            next: setup.next,
            duration: Duration::from_secs(setup.duration_secs),
            now: Moment::new(Duration::ZERO),
            revision: Revision::default(),
            cover_side: None,
        }
    }

    #[rstest]
    #[case::preload_due_point_arms_at_unity_speed(
        head_at(0, 1.0),
        lookahead(Setup { ab_loop: None, next: Some(a_track()), duration_secs: 100 }),
        Some(90)
    )]
    #[case::ab_b_point_arms_when_earlier_than_preload(
        head_at(0, 1.0),
        lookahead(Setup { ab_loop: Some((5, 20)), next: Some(a_track()), duration_secs: 100 }),
        Some(20)
    )]
    #[case::double_speed_halves_the_wait(
        head_at(0, 2.0),
        lookahead(Setup { ab_loop: None, next: Some(a_track()), duration_secs: 100 }),
        Some(45)
    )]
    #[case::half_speed_doubles_the_wait(
        head_at(0, 0.5),
        lookahead(Setup { ab_loop: None, next: Some(a_track()), duration_secs: 100 }),
        Some(180)
    )]
    #[case::a_past_decision_is_not_armed(
        head_at(95, 1.0),
        lookahead(Setup { ab_loop: None, next: Some(a_track()), duration_secs: 100 }),
        None
    )]
    #[case::no_next_track_skips_the_preload_point(
        head_at(0, 1.0),
        lookahead(Setup { ab_loop: None, next: None, duration_secs: 100 }),
        None
    )]
    fn next_decision_arms_the_earlier_of_preload_or_ab(
        #[case] playhead: Playhead,
        #[case] lookahead: Lookahead,
        #[case] expected_secs: Option<u64>,
    ) {
        let delay = next_decision(playhead, &lookahead);
        assert_eq!(delay, expected_secs.map(Duration::from_secs));
    }
}
