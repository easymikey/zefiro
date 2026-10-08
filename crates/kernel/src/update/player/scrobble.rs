use std::time::Duration;

use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        player::Player,
        revision::Revision,
        server::{Download, MediaFetch, PlayReport, Scrobble},
        time::Moment,
    },
    message::Timer,
    update::{
        machine::Unhandled,
        player::events::{PlaybackParts, duration_of},
    },
};

pub const SCROBBLE_AFTER: Duration = Duration::from_secs(240);

pub(crate) fn report(media_fetch: &MediaFetch, scrobble: Scrobble) -> Effect {
    Effect::Remote(RemoteCmd::Report {
        session: media_fetch.session.clone(),
        play_report: PlayReport {
            server_name: media_fetch.server_name.clone(),
            server_track_id: media_fetch.server_track_id.clone(),
            scrobble,
        },
    })
}

pub(crate) fn threshold(player: &Player) -> Duration {
    let half = duration_of(player) / 2;
    if half.is_zero() {
        SCROBBLE_AFTER
    } else {
        half.min(SCROBBLE_AFTER)
    }
}

fn media_fetch<'a>(
    player: &Player,
    downloads: &'a [Download],
) -> Option<&'a MediaFetch> {
    player.current().and_then(|track| {
        downloads
            .iter()
            .map(|download| &download.media_fetch)
            .find(|media_fetch| track.holds(media_fetch))
    })
}

pub(crate) fn scrobble(
    playback_parts: &mut PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let player = &*playback_parts.player;
    if !(playback_parts.revisions.scrobble == Some(revision)
        && media_fetch(player, playback_parts.downloads).is_some())
    {
        return Err(Unhandled);
    }
    match player {
        Player::Paused { .. } | Player::Stopped => Err(Unhandled),
        Player::Playing { .. } | Player::Loading(..) => {
            Ok(scrobble_timer(playback_parts, revision, now))
        }
    }
}

pub(crate) fn now_playing(playback_parts: &mut PlaybackParts<'_>, now: Moment) -> Cmd {
    let Some(media_fetch) =
        media_fetch(playback_parts.player, playback_parts.downloads)
    else {
        return Cmd::none();
    };
    let revision = media_fetch.revision;
    Cmd::from(report(media_fetch, Scrobble::NowPlaying)).then(scrobble_timer(
        playback_parts,
        revision,
        now,
    ))
}

pub(crate) fn scrobble_timer(
    playback_parts: &mut PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Cmd {
    let player = &*playback_parts.player;
    let Some(media_fetch) = media_fetch(player, playback_parts.downloads) else {
        return Cmd::none();
    };
    let rest = threshold(player).saturating_sub(player.position_at(now));
    if rest.is_zero() {
        playback_parts.revisions.scrobble = None;
        report(media_fetch, Scrobble::Played(now)).into()
    } else {
        playback_parts.revisions.scrobble = Some(revision);
        Effect::After {
            delay: rest.div_f32(playback_parts.transport.speed.get()),
            timer: Timer::Scrobble(revision),
        }
        .into()
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect, RemoteCmd},
        domain::{
            bounded::Bounded,
            model::Model,
            player::{PausedBy, Player},
            playhead::Playhead,
            revision::{Revision, Revisions},
            server::{
                Account,
                CacheKey,
                Download,
                Endpoint,
                MediaFetch,
                PlayReport,
                Scrobble,
                Server,
                ServerName,
                ServerStatus,
                ServerTrackId,
                Session,
                UserName,
            },
            speed::Speed,
            time::Moment,
            track::{AudioFormat, Tags, Track, TrackParts, TrackSource},
        },
        message::{Message, Timer},
        update::{
            machine::Unhandled,
            playback_parts,
            player::{
                PlayerMessage,
                scrobble::SCROBBLE_AFTER,
                stamp::{Anchor, Stamp},
                start,
                update_player,
            },
            update,
        },
    };

    fn a_track() -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: "/tmp/next.flac".into(),
            duration: Duration::from_secs(1),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        }))
    }

    fn now() -> Moment {
        Moment::new(Duration::from_secs(1_000))
    }

    fn playing(track: Arc<Track>) -> Player {
        Player::Playing {
            track,
            playhead: Playhead::anchored(Duration::ZERO, now(), Speed::default()),
            preloaded: None,
        }
    }

    fn server_track(secs: u64) -> Arc<Track> {
        Arc::new(
            Track::from(TrackSource::Server {
                server_name: ServerName::new("home"),
                server_track_id: ServerTrackId::new("tr-1"),
            })
            .with_duration(Duration::from_secs(secs)),
        )
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

    fn report(scrobble: Scrobble) -> Effect {
        Effect::Remote(RemoteCmd::Report {
            session: session(),
            play_report: PlayReport {
                server_name: ServerName::new("home"),
                server_track_id: ServerTrackId::new("tr-1"),
                scrobble,
            },
        })
    }

    fn scrobbling(player: Player) -> Model {
        let server_name = ServerName::new("home");
        let server_track_id = ServerTrackId::new("tr-1");
        Model {
            downloads: vec![Download {
                media_fetch: MediaFetch {
                    cache_key: CacheKey::new(&server_name, &server_track_id, "flac"),
                    server_name,
                    server_track_id,
                    session: session(),
                    first_byte: 0,
                    revision: Revision::default().next(),
                },
                fetched: None,
            }],
            revisions: Revisions {
                scrobble: Some(Revision::default().next()),
                ..Revisions::default()
            },
            ..online(player)
        }
    }

    fn fired(model: &mut Model, revision: Revision) -> Result<Vec<Effect>, Unhandled> {
        update(model, Message::Elapsed(Timer::Scrobble(revision)), now())
    }

    #[rstest]
    #[case::half_of_a_short_track(100, Duration::from_secs(50))]
    #[case::four_minutes_of_a_long_track(600, SCROBBLE_AFTER)]
    #[case::four_minutes_of_an_unknown_length(0, SCROBBLE_AFTER)]
    fn a_server_track_start_reports_now_playing_and_arms_the_scrobble(
        #[case] secs: u64,
        #[case] delay: Duration,
    ) {
        let mut model = online(Player::Stopped);

        let (Ok(cmd) | Err(cmd)) =
            start(&mut playback_parts(&mut model), server_track(secs), now());

        let effects: Vec<Effect> = cmd.effects().cloned().collect();
        assert!(effects.contains(&report(Scrobble::NowPlaying)));
        assert!(effects.contains(&Effect::After {
            delay,
            timer: Timer::Scrobble(Revision::default().next()),
        }));
    }

    #[rstest]
    #[case::played_past_half(Duration::from_secs(50), vec![report(Scrobble::Played(now()))])]
    #[case::short_of_half_rearms_for_the_rest(Duration::from_secs(10), vec![Effect::After { delay: Duration::from_secs(40), timer: Timer::Scrobble(Revision::default().next()) }])]
    fn the_scrobble_timer_reports_played_once_the_threshold_is_reached(
        #[case] position: Duration,
        #[case] expected: Vec<Effect>,
    ) {
        let mut model = scrobbling(Player::Playing {
            track: server_track(100),
            playhead: Playhead::anchored(position, now(), Speed::default()),
            preloaded: None,
        });

        assert_eq!(fired(&mut model, Revision::default().next()), Ok(expected));
    }

    #[rstest]
    #[case::short_of_the_threshold(Duration::from_secs(20))]
    #[case::at_the_threshold(Duration::from_secs(50))]
    #[case::past_the_threshold(Duration::from_secs(60))]
    fn a_scrobble_timer_while_paused_arms_nothing(#[case] position: Duration) {
        let mut model = scrobbling(Player::Paused {
            track: server_track(100),
            position,
            by: PausedBy::Listener,
        });

        assert_eq!(
            fired(&mut model, Revision::default().next()),
            Err(Unhandled)
        );
    }

    fn later(secs: u64) -> Moment {
        Moment::new(Duration::from_secs(1_000 + secs))
    }

    fn scrobble_timers(cmd: &Cmd) -> Vec<Effect> {
        cmd.effects()
            .filter(|effect| {
                matches!(
                    effect,
                    Effect::After {
                        timer: Timer::Scrobble(_),
                        ..
                    }
                )
            })
            .cloned()
            .collect()
    }

    fn played(speed: Speed) -> (Model, Vec<Effect>) {
        let mut model = online(Player::Stopped);
        model.transport.speed = speed;
        let (Ok(cmd) | Err(cmd)) =
            start(&mut playback_parts(&mut model), server_track(100), now());
        let loaded = update_player(
            &mut playback_parts(&mut model),
            PlayerMessage::Loaded {
                duration: None,
                anchor: Anchor {
                    started_at: now(),
                    speed,
                },
            },
            now(),
        );
        assert!(loaded.is_ok());
        (model, scrobble_timers(&cmd))
    }

    fn toggled(model: &mut Model, moment: Moment) -> Vec<Effect> {
        let stamp = Stamp::pending(&model.transport, &model.revisions, moment);
        let cmd = update_player(
            &mut playback_parts(model),
            PlayerMessage::Toggle {
                current: None,
                stamp,
            },
            moment,
        )
        .unwrap();
        scrobble_timers(&cmd)
    }

    #[test]
    fn a_pause_just_before_the_threshold_arms_the_scrobble_only_on_resume() {
        let (mut model, _armed) = played(Speed::default());
        assert_eq!(toggled(&mut model, later(49)), Vec::new());

        assert_eq!(
            update(
                &mut model,
                Message::Elapsed(Timer::Scrobble(Revision::default().next())),
                later(50),
            ),
            Err(Unhandled)
        );
        let resumed = toggled(&mut model, later(60));

        let revision = model.revisions.effects;
        assert_eq!(
            resumed,
            vec![Effect::After {
                delay: Duration::from_secs(1),
                timer: Timer::Scrobble(revision),
            }]
        );
        assert_eq!(
            update(
                &mut model,
                Message::Elapsed(Timer::Scrobble(revision)),
                later(61)
            ),
            Ok(vec![report(Scrobble::Played(later(61)))])
        );
    }

    #[test]
    fn a_seek_back_after_the_report_then_a_pause_and_a_resume_arm_nothing() {
        let (mut model, _armed) = played(Speed::default());
        assert_eq!(
            update(
                &mut model,
                Message::Elapsed(Timer::Scrobble(Revision::default().next())),
                later(50),
            ),
            Ok(vec![report(Scrobble::Played(later(50)))])
        );
        let player_message = PlayerMessage::Seek {
            target: Duration::ZERO,
            now: later(51),
        };
        assert!(
            update_player(&mut playback_parts(&mut model), player_message, later(51))
                .is_ok()
        );

        assert_eq!(toggled(&mut model, later(52)), Vec::new());
        assert_eq!(toggled(&mut model, later(53)), Vec::new());
    }

    #[test]
    fn a_seek_past_the_threshold_while_paused_reports_played_on_resume() {
        let (mut model, _armed) = played(Speed::default());
        assert_eq!(toggled(&mut model, later(49)), Vec::new());
        assert_eq!(
            update(
                &mut model,
                Message::Elapsed(Timer::Scrobble(Revision::default().next())),
                later(50),
            ),
            Err(Unhandled)
        );
        let player_message = PlayerMessage::Seek {
            target: Duration::from_secs(60),
            now: later(51),
        };
        assert!(
            update_player(&mut playback_parts(&mut model), player_message, later(51))
                .is_ok()
        );
        let stamp = Stamp::pending(&model.transport, &model.revisions, later(52));

        let cmd = update_player(
            &mut playback_parts(&mut model),
            PlayerMessage::Toggle {
                current: None,
                stamp,
            },
            later(52),
        )
        .unwrap();

        assert!(
            cmd.effects()
                .any(|effect| effect == &report(Scrobble::Played(later(52))))
        );
        assert_eq!(model.revisions.scrobble, None);
    }

    #[test]
    fn at_half_speed_the_scrobble_fires_once_after_twice_the_track_time() {
        let (mut model, armed) = played(Speed::clamped(0.5));

        assert_eq!(
            armed,
            vec![Effect::After {
                delay: Duration::from_secs(100),
                timer: Timer::Scrobble(Revision::default().next()),
            }]
        );
        assert_eq!(
            update(
                &mut model,
                Message::Elapsed(Timer::Scrobble(Revision::default().next())),
                later(100),
            ),
            Ok(vec![report(Scrobble::Played(later(100)))])
        );
    }

    #[rstest]
    #[case::a_skipped_track(scrobbling(playing(a_track())), Revision::default().next())]
    #[case::a_stopped_player(scrobbling(Player::Stopped), Revision::default().next())]
    #[case::a_stale_revision(scrobbling(playing(server_track(100))), Revision::default().next().next())]
    fn a_scrobble_timer_of_a_track_no_longer_playing_is_refused(
        #[case] mut model: Model,
        #[case] revision: Revision,
    ) {
        assert_eq!(fired(&mut model, revision), Err(Unhandled));
    }
}
