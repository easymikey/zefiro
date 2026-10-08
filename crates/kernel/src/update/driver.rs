use std::sync::Arc;

use crate::{
    cmd::{AudioCmd, Cmd, Effect, Playback, TrackLoad},
    domain::{
        driver::{DriverError, DriverName, DriverStatus, Drivers},
        player::Player,
        revision::{Revision, Revisions},
        server::Download,
        supervision::{Decision, Supervision, decide_restart},
        time::Moment,
        toast::Toast,
        track::Track,
        transport::Transport,
    },
    message::{DriverEvent, Message, PlaybackRequest},
    update::machine::{Machine, Unhandled},
};

pub(crate) struct DriverDeath {
    pub(crate) driver_name: DriverName,
    pub(crate) error: DriverError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatusMessage {
    Died(DriverError),
    Stopped,
    Full(DriverName),
}

impl Machine for DriverStatus {
    type Message = DriverStatusMessage;
    type Effect = Cmd;

    fn transition(
        &mut self,
        driver_status_message: DriverStatusMessage,
    ) -> Result<Cmd, Unhandled> {
        match (&*self, driver_status_message) {
            (DriverStatus::Running, DriverStatusMessage::Died(error)) => {
                *self = DriverStatus::Dead(error);
                Ok(Cmd::none())
            }
            (
                DriverStatus::Running | DriverStatus::Dead(_),
                DriverStatusMessage::Stopped,
            ) => {
                *self = DriverStatus::Stopped;
                Ok(Cmd::none())
            }
            (DriverStatus::Running, DriverStatusMessage::Full(driver)) => {
                Ok(Cmd::message(Message::Toast(Toast::info(format!(
                    "The {driver} driver is falling behind"
                )))))
            }
            (
                DriverStatus::Dead(_),
                DriverStatusMessage::Died(_) | DriverStatusMessage::Full(..),
            )
            | (
                DriverStatus::Stopped,
                DriverStatusMessage::Died(_)
                | DriverStatusMessage::Stopped
                | DriverStatusMessage::Full(..),
            ) => Err(Unhandled),
        }
    }
}

pub(crate) fn update(
    drivers: &mut Drivers,
    driver_name: DriverName,
    event: DriverEvent,
) -> Result<Cmd, Unhandled> {
    let driver_status_message = match event {
        DriverEvent::Died(error) => DriverStatusMessage::Died(error),
        DriverEvent::Stopped => DriverStatusMessage::Stopped,
        DriverEvent::Full => DriverStatusMessage::Full(driver_name),
    };
    drivers
        .record_mut(driver_name)
        .status
        .transition(driver_status_message)
}

pub(crate) fn died(
    drivers: &mut Drivers,
    driver_death: DriverDeath,
    now: Moment,
) -> Result<(Decision, Cmd), Unhandled> {
    let DriverDeath { driver_name, error } = driver_death;
    let died = update(drivers, driver_name, DriverEvent::Died(error))?;
    Ok((restart_if_allowed(drivers, driver_name, now), died))
}

fn restart_if_allowed(
    drivers: &mut Drivers,
    driver_name: DriverName,
    now: Moment,
) -> Decision {
    let decision = decide_restart(
        Supervision::standard(driver_name),
        &drivers.record(driver_name).restarts,
        now,
    );
    match decision {
        Decision::Restart => {
            let restarting = drivers.record_mut(driver_name);
            restarting.status = DriverStatus::Running;
            restarting.restarts.record(now);
        }
        Decision::Degrade(_) => {}
    }
    decision
}

pub(crate) struct ResumeParts<'a> {
    pub(crate) player: &'a Player,
    pub(crate) transport: &'a Transport,
    pub(crate) downloads: &'a [Download],
    pub(crate) revisions: &'a mut Revisions,
}

pub(crate) fn resume_driver(
    parts: ResumeParts<'_>,
    driver_name: DriverName,
    now: Moment,
) -> Cmd {
    let ResumeParts {
        player,
        transport,
        downloads,
        revisions,
    } = parts;
    let Some((track, playback)) = (match driver_name {
        DriverName::Audio => resumed(player),
        DriverName::Library
        | DriverName::Config
        | DriverName::Macos
        | DriverName::Remote => None,
    }) else {
        return Cmd::none();
    };
    let audio_cmds = [
        AudioCmd::SetPlayback(playback),
        AudioCmd::SetSpeed(transport.speed),
    ];
    let candidate = revisions.effects.next();
    let [download, preloaded] = [Some(track), player.preloaded()].map(|held| {
        held.and_then(|held| {
            downloads
                .iter()
                .find(|download| held.holds(&download.media_fetch))
        })
    });
    let Some(load) = track_load(track, download, candidate) else {
        return download.map_or_else(
            || Cmd::message(Message::Playback(PlaybackRequest::Stop)),
            |_download| Cmd::from_iter(audio_cmds.map(Effect::Audio)),
        );
    };
    let preload = player
        .preloaded()
        .and_then(|next| track_load(next, preloaded, candidate));
    if download.is_none() || (preloaded.is_none() && preload.is_some()) {
        revisions.effects = candidate;
    }
    Cmd::from_iter(
        [
            AudioCmd::Load(load),
            AudioCmd::Seek(player.position_at(now)),
        ]
        .into_iter()
        .chain(audio_cmds)
        .chain(preload.map(AudioCmd::Preload))
        .map(Effect::Audio),
    )
}

fn resumed(player: &Player) -> Option<(&Arc<Track>, Playback)> {
    match player {
        Player::Playing {
            track,
            playhead: _playhead,
            preloaded: _preloaded,
        } => Some((track, Playback::Playing)),
        Player::Loading(track) => Some((track, Playback::Playing)),
        Player::Paused {
            track,
            position: _position,
            by: _by,
        } => Some((track, Playback::Paused)),
        Player::Stopped => None,
    }
}

fn track_load(
    track: &Track,
    download: Option<&Download>,
    revision: Revision,
) -> Option<TrackLoad> {
    download.map_or_else(
        || TrackLoad::for_track(track, revision),
        |download| TrackLoad::fetched(track, download),
    )
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
        cmd::{AudioCmd, Cmd, Effect, Playback, TrackLoad},
        domain::{
            bounded::Bounded,
            driver::{DriverError, DriverName, DriverStatus, Drivers},
            player::{PausedBy, Player},
            playhead::Playhead,
            revision::{Revision, Revisions},
            server::{
                CacheKey,
                Download,
                Endpoint,
                Fetched,
                MediaFetch,
                START_MARGIN,
                ServerName,
                ServerTrackId,
                Session,
            },
            speed::Speed,
            supervision::Decision,
            time::Moment,
            toast::Toast,
            track::{Track, TrackSource},
            transport::Transport,
        },
        message::{Message, PlaybackRequest},
        update::{
            driver::{
                DriverDeath,
                DriverStatusMessage,
                ResumeParts,
                died,
                resume_driver,
            },
            machine::{Machine, Unhandled},
        },
    };

    fn dead() -> DriverStatus {
        DriverStatus::Dead(DriverError::Panicked)
    }

    fn panicked() -> DriverStatusMessage {
        DriverStatusMessage::Died(DriverError::Panicked)
    }

    fn filled() -> DriverStatusMessage {
        DriverStatusMessage::Full(DriverName::Audio)
    }

    fn full() -> Result<Cmd, Unhandled> {
        Ok(Cmd::message(Message::Toast(Toast::info(
            "The audio driver is falling behind".to_string(),
        ))))
    }

    struct LifeRow {
        driver_status: DriverStatus,
        driver_status_message: DriverStatusMessage,
        next: DriverStatus,
        result: Result<Cmd, Unhandled>,
    }

    #[rstest]
    #[case::running_dies(LifeRow {
        driver_status: DriverStatus::Running,
        driver_status_message: panicked(),
        next: dead(),
        result: Ok(Cmd::none()),
    })]
    #[case::running_stops(LifeRow {
        driver_status: DriverStatus::Running,
        driver_status_message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Ok(Cmd::none()),
    })]
    #[case::running_is_full(LifeRow {
        driver_status: DriverStatus::Running,
        driver_status_message: filled(),
        next: DriverStatus::Running,
        result: full(),
    })]
    #[case::dead_refuses_a_second_death(LifeRow {
        driver_status: dead(),
        driver_status_message: panicked(),
        next: dead(),
        result: Err(Unhandled),
    })]
    #[case::dead_stops(LifeRow {
        driver_status: dead(),
        driver_status_message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Ok(Cmd::none()),
    })]
    #[case::dead_refuses_full(LifeRow {
        driver_status: dead(),
        driver_status_message: filled(),
        next: dead(),
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_death(LifeRow {
        driver_status: DriverStatus::Stopped,
        driver_status_message: panicked(),
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_a_second_stop(LifeRow {
        driver_status: DriverStatus::Stopped,
        driver_status_message: DriverStatusMessage::Stopped,
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    #[case::stopped_refuses_full(LifeRow {
        driver_status: DriverStatus::Stopped,
        driver_status_message: filled(),
        next: DriverStatus::Stopped,
        result: Err(Unhandled),
    })]
    fn a_driver_lives_through_its_table(#[case] row: LifeRow) {
        let mut status = row.driver_status;
        let result = status.transition(row.driver_status_message);
        assert_eq!(status, row.next);
        assert_eq!(result, row.result);
    }

    #[rstest]
    #[case::a_dead_driver(dead())]
    #[case::a_stopped_driver(DriverStatus::Stopped)]
    fn a_death_the_status_refuses_is_refused_and_not_recorded(
        #[case] status: DriverStatus,
    ) {
        let mut drivers = Drivers::default();
        drivers.record_mut(DriverName::Audio).status = status;
        let before = drivers.clone();

        let death = died(
            &mut drivers,
            DriverDeath {
                driver_name: DriverName::Audio,
                error: DriverError::Panicked,
            },
            Moment::default(),
        );

        assert_eq!(death, Err(Unhandled));
        assert_eq!(drivers, before);
    }

    #[test]
    fn a_dead_driver_restarts_and_the_restart_is_recorded() {
        let mut drivers = Drivers::default();
        let before = drivers.clone();

        let death = died(
            &mut drivers,
            DriverDeath {
                driver_name: DriverName::Audio,
                error: DriverError::Panicked,
            },
            Moment::default(),
        );

        assert_eq!(death, Ok((Decision::Restart, Cmd::none())));
        assert_eq!(drivers.status(DriverName::Audio), &DriverStatus::Running);
        assert_ne!(
            drivers.record(DriverName::Audio).restarts,
            before.record(DriverName::Audio).restarts
        );
    }

    struct ResumeRow {
        player: Player,
        downloads: Vec<Download>,
        driver_name: DriverName,
        revisions: Revisions,
        cmd: Cmd,
    }

    fn track() -> Arc<Track> {
        Arc::new(Track::listed(Path::new("/music/a.flac")))
    }

    fn next_track() -> Arc<Track> {
        Arc::new(Track::listed(Path::new("/music/b.flac")))
    }

    fn server_track(id: &str) -> Arc<Track> {
        Arc::new(Track::from(TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new(id),
        }))
    }

    fn download(id: &str, revision: Revision, downloaded: u64) -> Download {
        let server_name = ServerName::new("home");
        let server_track_id = ServerTrackId::new(id);
        Download {
            media_fetch: MediaFetch {
                cache_key: CacheKey::new(&server_name, &server_track_id, "flac"),
                server_name,
                server_track_id,
                session: Session::new(
                    Endpoint::parse("https://music.example.com").unwrap(),
                    "u=ann&t=token&s=salt",
                ),
                first_byte: 0,
                revision,
            },
            fetched: Some(Fetched {
                media_path: PathBuf::from(format!("/cache/home/{id}.flac")),
                downloaded,
                byte_len: 4 * START_MARGIN,
            }),
        }
    }

    fn fetch_revision() -> Revision {
        Revision::default().next().next().next()
    }

    fn served(id: &str) -> TrackLoad {
        TrackLoad::fetched(
            &server_track(id),
            &download(id, fetch_revision(), START_MARGIN),
        )
        .unwrap()
    }

    fn reloaded(position: Duration, playback: Playback) -> Cmd {
        Cmd::from_iter([
            Effect::Audio(AudioCmd::Load(
                TrackLoad::for_track(&track(), Revisions::default().issue_effect())
                    .unwrap(),
            )),
            Effect::Audio(AudioCmd::Seek(position)),
            Effect::Audio(AudioCmd::SetPlayback(playback)),
            Effect::Audio(AudioCmd::SetSpeed(Speed::default())),
        ])
    }

    #[rstest]
    #[case::a_playing_track_resumes_playing(ResumeRow {
        player: Player::Playing {
            track: track(),
            playhead: Playhead::anchored(
                Duration::from_secs(5),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        },
        downloads: Vec::new(),
        driver_name: DriverName::Audio,
        revisions: Revisions {
            effects: Revision::default().next(),
            ..Revisions::default()
        },
        cmd: reloaded(Duration::from_secs(5), Playback::Playing),
    })]
    #[case::a_loading_track_resumes_playing(ResumeRow {
        player: Player::Loading(track()),
        downloads: Vec::new(),
        driver_name: DriverName::Audio,
        revisions: Revisions {
            effects: Revision::default().next(),
            ..Revisions::default()
        },
        cmd: reloaded(Duration::ZERO, Playback::Playing),
    })]
    #[case::a_paused_track_resumes_paused(ResumeRow {
        player: Player::Paused {
            track: track(),
            position: Duration::from_secs(3),
            by: PausedBy::Listener,
        },
        downloads: Vec::new(),
        driver_name: DriverName::Audio,
        revisions: Revisions {
            effects: Revision::default().next(),
            ..Revisions::default()
        },
        cmd: reloaded(Duration::from_secs(3), Playback::Paused),
    })]
    #[case::a_stopped_player_resumes_nothing(ResumeRow {
        player: Player::Stopped,
        downloads: Vec::new(),
        driver_name: DriverName::Audio,
        revisions: Revisions::default(),
        cmd: Cmd::none(),
    })]
    #[case::a_library_restart_resumes_nothing(ResumeRow {
        player: Player::Loading(track()),
        downloads: Vec::new(),
        driver_name: DriverName::Library,
        revisions: Revisions::default(),
        cmd: Cmd::none(),
    })]
    #[case::a_playing_server_track_resumes(ResumeRow {
        player: Player::Playing {
            track: server_track("tr-1"),
            playhead: Playhead::anchored(
                Duration::from_secs(5),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: Some(server_track("tr-2")),
        },
        downloads: vec![
            download("tr-1", fetch_revision(), START_MARGIN),
            download("tr-2", fetch_revision(), START_MARGIN),
        ],
        driver_name: DriverName::Audio,
        revisions: Revisions::default(),
        cmd: Cmd::from_iter([
            Effect::Audio(AudioCmd::Load(served("tr-1"))),
            Effect::Audio(AudioCmd::Seek(Duration::from_secs(5))),
            Effect::Audio(AudioCmd::SetPlayback(Playback::Playing)),
            Effect::Audio(AudioCmd::SetSpeed(Speed::default())),
            Effect::Audio(AudioCmd::Preload(served("tr-2"))),
        ]),
    })]
    #[case::a_local_track_resumes_before_a_server_successor(ResumeRow {
        player: Player::Playing {
            track: track(),
            playhead: Playhead::anchored(
                Duration::from_secs(5),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: Some(server_track("tr-2")),
        },
        downloads: vec![download("tr-2", fetch_revision(), START_MARGIN)],
        driver_name: DriverName::Audio,
        revisions: Revisions {
            effects: Revision::default().next(),
            ..Revisions::default()
        },
        cmd: reloaded(Duration::from_secs(5), Playback::Playing)
            .then(Cmd::effect(Effect::Audio(AudioCmd::Preload(served("tr-2"))))),
    })]
    #[case::a_server_track_resumes_before_a_local_successor(ResumeRow {
        player: Player::Playing {
            track: server_track("tr-1"),
            playhead: Playhead::anchored(
                Duration::from_secs(5),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: Some(next_track()),
        },
        downloads: vec![download("tr-1", fetch_revision(), START_MARGIN)],
        driver_name: DriverName::Audio,
        revisions: Revisions {
            effects: Revision::default().next(),
            ..Revisions::default()
        },
        cmd: Cmd::from_iter([
            Effect::Audio(AudioCmd::Load(served("tr-1"))),
            Effect::Audio(AudioCmd::Seek(Duration::from_secs(5))),
            Effect::Audio(AudioCmd::SetPlayback(Playback::Playing)),
            Effect::Audio(AudioCmd::SetSpeed(Speed::default())),
            Effect::Audio(AudioCmd::Preload(
                TrackLoad::for_track(
                    &next_track(),
                    Revisions::default().issue_effect(),
                )
                .unwrap(),
            )),
        ]),
    })]
    #[case::a_loading_server_track_waits_for_its_download(ResumeRow {
        player: Player::Loading(server_track("tr-1")),
        downloads: vec![download("tr-1", fetch_revision(), 0)],
        driver_name: DriverName::Audio,
        revisions: Revisions::default(),
        cmd: Cmd::from_iter([
            Effect::Audio(AudioCmd::SetPlayback(Playback::Playing)),
            Effect::Audio(AudioCmd::SetSpeed(Speed::default())),
        ]),
    })]
    #[case::a_server_track_without_its_download_stops(ResumeRow {
        player: Player::Paused {
            track: server_track("tr-1"),
            position: Duration::from_secs(3),
            by: PausedBy::Listener,
        },
        downloads: Vec::new(),
        driver_name: DriverName::Audio,
        revisions: Revisions::default(),
        cmd: Cmd::message(Message::Playback(PlaybackRequest::Stop)),
    })]
    fn an_audio_restart_resumes_the_player(#[case] row: ResumeRow) {
        let ResumeRow {
            player,
            downloads,
            driver_name,
            revisions: expected_revisions,
            cmd,
        } = row;
        let mut revisions = Revisions::default();
        let effects = resume_driver(
            ResumeParts {
                player: &player,
                transport: &Transport::default(),
                downloads: &downloads,
                revisions: &mut revisions,
            },
            driver_name,
            Moment::default(),
        );
        assert_eq!((effects, revisions), (cmd, expected_revisions));
    }

    #[test]
    fn an_audio_restart_resends_the_speed_and_the_preload() {
        let speed = Speed::clamped(1.5);
        let player = Player::Playing {
            track: track(),
            playhead: Playhead::anchored(
                Duration::from_secs(5),
                Moment::default(),
                speed,
            ),
            preloaded: Some(next_track()),
        };
        let revision = Revisions::default().issue_effect();
        let effects = resume_driver(
            ResumeParts {
                player: &player,
                transport: &Transport {
                    speed,
                    ..Transport::default()
                },
                downloads: &[],
                revisions: &mut Revisions::default(),
            },
            DriverName::Audio,
            Moment::default(),
        );
        assert_eq!(
            effects,
            Cmd::from_iter([
                Effect::Audio(AudioCmd::Load(
                    TrackLoad::for_track(&track(), revision).unwrap()
                )),
                Effect::Audio(AudioCmd::Seek(Duration::from_secs(5))),
                Effect::Audio(AudioCmd::SetPlayback(Playback::Playing)),
                Effect::Audio(AudioCmd::SetSpeed(speed)),
                Effect::Audio(AudioCmd::Preload(
                    TrackLoad::for_track(&next_track(), revision,).unwrap()
                )),
            ])
        );
    }
}
