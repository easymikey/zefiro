use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, Effect, GrowingMedia, Media, RemoteCmd, TrackLoad},
    domain::{
        cue::{Cue, PlaybackChange},
        driver::{DriverError, DriverName},
        model::Model,
        player::Player,
        playhead::Playhead,
        revision::Revision,
        server::{
            Account,
            CacheKey,
            Endpoint,
            Fetched,
            MediaFetch,
            PlayReport,
            START_MARGIN,
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
        toast::Toast,
        track::{AudioFormat, Tags, Track, TrackParts, TrackSource},
    },
    message::{AudioEvent, DriverEvent, Message, PlaybackRequest, RemoteEvent, Timer},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    model_with_dated_tracks,
    playing_model,
    update::{send, update},
};

fn sent(model: &mut Model, message: Message) -> Cmd {
    update(model, message, Moment::default()).unwrap()
}

fn sent_at(model: &mut Model, message: Message, now: Moment) -> Cmd {
    update(model, message, now).unwrap()
}

fn scheduled(cmd: &Cmd) -> Vec<Timer> {
    cmd.effects()
        .filter_map(|effect| {
            if let Effect::After { timer: message, .. } = effect {
                Some(*message)
            } else {
                None
            }
        })
        .collect()
}

fn toast_shown(model: &mut Model, text: &str) -> Timer {
    let cmd = sent(model, Message::Toast(Toast::info(text)));
    let timers = scheduled(&cmd);
    assert!(matches!(timers.as_slice(), [Timer::Toast(_)]), "{timers:?}");
    timers[0]
}

fn sleep_cycled(model: &mut Model) -> Cmd {
    sent(model, Message::Playback(PlaybackRequest::CycleSleep))
}

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn position(position: Duration) -> Message {
    Message::Audio(AudioEvent::PositionReported {
        position,
        revision: Revision::default(),
    })
}

fn moment(millis: u64) -> Moment {
    Moment::new(Duration::from_millis(millis))
}

#[test]
fn a_seek_back_after_the_preload_arms_no_preload_point() {
    let mut model = model_with_dated_tracks(3);
    send(&mut model, Message::Playback(PlaybackRequest::Toggle));
    send(&mut model, Message::Audio(AudioEvent::Loaded(None)));
    send(&mut model, position(secs(95)));
    let mark = model.revisions.lookahead;
    send(&mut model, Message::Elapsed(Timer::Lookahead(mark)));
    assert!(matches!(
        model.player,
        Player::Playing {
            preloaded: Some(_),
            ..
        }
    ));

    let cmd = sent(
        &mut model,
        Message::Playback(PlaybackRequest::SeekTo(secs(10))),
    );

    assert!(scheduled(&cmd).is_empty(), "{cmd:?}");
}

fn sought(model: &mut Model, target: Duration, now: Moment) {
    assert!(
        update(
            model,
            Message::Playback(PlaybackRequest::SeekTo(target)),
            now
        )
        .is_ok()
    );
}

#[test]
fn a_report_from_before_a_seek_leaves_the_playhead_at_the_target() {
    let mut model = playing_model(3);
    sought(&mut model, secs(15), moment(1_000));

    assert!(update(&mut model, position(secs(1)), moment(1_100)).is_err());

    assert_eq!(
        model.player.position_at(moment(1_100)),
        Duration::from_millis(15_100)
    );
}

#[test]
fn the_playhead_runs_on_from_a_seek_before_any_report() {
    let mut model = playing_model(3);
    sought(&mut model, secs(15), moment(1_000));

    assert_eq!(
        model.player.position_at(moment(1_800)),
        Duration::from_millis(15_800)
    );
}

#[test]
fn a_report_after_a_seek_moves_the_playhead_back_by_at_most_its_lag() {
    let mut model = playing_model(3);
    sought(&mut model, secs(15), moment(1_000));

    assert!(
        update(
            &mut model,
            Message::Audio(AudioEvent::PositionReported {
                position: secs(13),
                revision: Revision::default().next(),
            }),
            moment(2_000),
        )
        .is_ok()
    );

    assert_eq!(
        model.player.position_at(moment(2_000)),
        Duration::from_millis(15_750)
    );
}

#[test]
fn a_report_after_an_audio_restart_while_stopped_is_taken() {
    let mut model = playing_model(3);
    sought(&mut model, secs(15), moment(1_000));
    for message in [
        Message::Playback(PlaybackRequest::Stop),
        Message::Driver {
            driver_name: DriverName::Audio,
            event: DriverEvent::Died(DriverError::Panicked),
        },
        Message::Playback(PlaybackRequest::Play),
        Message::Audio(AudioEvent::Loaded(None)),
    ] {
        assert!(update(&mut model, message, moment(1_100)).is_ok());
    }

    assert!(update(&mut model, position(secs(1)), moment(1_200)).is_ok());
}

#[test]
fn a_toast_timer_that_fires_early_keeps_the_toast_and_waits_again() {
    let mut model = Model::default();
    let timer = toast_shown(&mut model, "hello");

    let cmd = sent_at(&mut model, Message::Elapsed(timer), moment(4000));

    assert_eq!(
        cmd,
        Cmd::from(Effect::After {
            delay: secs(1),
            timer,
        })
    );
    assert_eq!(model.workspace.toasts.len(), 1);
}

#[test]
fn a_later_toast_shares_the_timer_and_expires_by_its_own_age() {
    let mut model = Model::default();
    let timer = toast_shown(&mut model, "first");
    let second = sent_at(
        &mut model,
        Message::Toast(Toast::info("second")),
        moment(3000),
    );
    assert_eq!(second, Cmd::from(Cue::ToastRaised));

    let first_gone = sent_at(&mut model, Message::Elapsed(timer), moment(5000));
    let titles: Vec<&str> = model
        .workspace
        .toasts
        .iter()
        .map(|toast| toast.title.as_str())
        .collect();
    assert_eq!(titles, ["second"]);
    assert_eq!(
        first_gone,
        Cmd::from_iter([
            Effect::Animate(Cue::ToastDismissed),
            Effect::After {
                delay: secs(3),
                timer,
            },
        ])
    );

    let second_gone = sent_at(&mut model, Message::Elapsed(timer), moment(8000));
    assert_eq!(second_gone, Cmd::from(Cue::ToastDismissed));
    assert!(model.workspace.toasts.is_empty());
}

#[test]
fn a_stale_toast_timer_changes_nothing() {
    let mut model = Model::default();
    let first = toast_shown(&mut model, "first");
    model.workspace.toasts.clear();
    let second = toast_shown(&mut model, "second");

    let stale = update(&mut model, Message::Elapsed(first), moment(9000));

    assert_ne!(first, second);
    assert_eq!(stale, Err(Unhandled));
    assert_eq!(model.workspace.toasts.len(), 1);
}

#[rstest]
#[case::over_a_playing_player_it_pauses_in_place(vec![], PlaybackChange::Pause.cued())]
#[case::over_a_paused_player_it_only_disarms(
    vec![Message::Playback(PlaybackRequest::Toggle)],
    Cmd::none()
)]
fn an_elapsed_sleep_timer_disarms(#[case] before: Vec<Message>, #[case] expected: Cmd) {
    let mut model = playing_model(3);
    let timer = scheduled(&sleep_cycled(&mut model))[0];
    before
        .into_iter()
        .for_each(|message| send(&mut model, message));

    let cmd = sent(&mut model, Message::Elapsed(timer));
    let again = update(&mut model, Message::Elapsed(timer), Moment::default());

    assert_eq!(cmd, expected);
    assert!(matches!(model.player, Player::Paused { .. }));
    assert_eq!(model.transport.sleep_timer, None);
    assert_eq!(again, Err(Unhandled));
}

#[rstest]
#[case::a_rearmed_one_ignores_the_first(|_| 2, |_| 0, Some(1))]
#[case::a_cancelled_one_ignores_the_last(|presets| presets + 1, |presets| presets - 1, None)]
fn a_stale_sleep_timer_changes_nothing(
    #[case] cycles: fn(usize) -> usize,
    #[case] fired: fn(usize) -> usize,
    #[case] preset_index: Option<usize>,
) {
    let mut model = playing_model(3);
    let presets = model.settings.audio_settings.sleep_presets.as_slice().len();
    let armed_timers: Vec<Timer> = (0..cycles(presets))
        .flat_map(|_| scheduled(&sleep_cycled(&mut model)))
        .collect();

    let cmd = update(
        &mut model,
        Message::Elapsed(armed_timers[fired(presets)]),
        Moment::default(),
    );

    assert_eq!(cmd, Err(Unhandled));
    assert!(model.player.is_playing());
    assert_eq!(
        model
            .transport
            .sleep_timer
            .map(|timer| timer.preset_index.get()),
        preset_index
    );
}

#[test]
fn a_stale_mark_is_ignored() {
    let mut model = playing_model(3);
    let armed = sent(&mut model, position(secs(50)));
    let stale = scheduled(&armed)[0];
    drop(sent(&mut model, position(secs(60))));

    let cmd = update(&mut model, Message::Elapsed(stale), Moment::default());

    assert_eq!(cmd, Err(Unhandled));
}

fn home_session() -> Session {
    Session::new(
        Endpoint::parse("https://music.example.com").unwrap(),
        "u=ann",
    )
}

fn incoming_track() -> Arc<Track> {
    Arc::new(Track::from(TrackSource::Server {
        server_name: ServerName::new("home"),
        server_track_id: ServerTrackId::new("tr-1"),
    }))
}

fn incoming(revision: Revision, first_byte: u64) -> MediaFetch {
    let server_name = ServerName::new("home");
    let server_track_id = ServerTrackId::new("tr-1");
    MediaFetch {
        cache_key: CacheKey::new(&server_name, &server_track_id, ""),
        server_name,
        server_track_id,
        session: home_session(),
        first_byte,
        revision,
    }
}

fn due() -> (Model, Result<Vec<Effect>, Unhandled>) {
    let mut model = Model {
        servers: vec![Server {
            account: Account {
                server_name: ServerName::new("home"),
                endpoint: Endpoint::parse("https://music.example.com").unwrap(),
                user_name: UserName::new("ann").unwrap(),
            },
            server_status: ServerStatus::Online(home_session()),
        }],
        player: Player::Playing {
            track: Arc::new(Track::new(TrackParts {
                path: "/tmp/track0.flac".into(),
                duration: Duration::from_secs(100),
                tags: Tags::default(),
                audio_format: AudioFormat::default(),
            })),
            playhead: Playhead::anchored(
                Duration::from_secs(95),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        },
        ..Model::default()
    };
    model.playlist.tracks = vec![incoming_track()];
    model.queue = vec![incoming_track()];
    let mark = model.revisions.lookahead;
    let effects = kernel::update::update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    );
    (model, effects)
}

fn prefetched(model: &Model) -> Revision {
    model
        .downloads
        .first()
        .map_or(Revision::default(), |download| {
            download.media_fetch.revision
        })
}

fn grown(
    model: &mut Model,
    revision: Revision,
    downloaded: u64,
) -> Result<Vec<Effect>, Unhandled> {
    let fetched = Fetched {
        media_path: PathBuf::from("/cache/home/tr-1.part"),
        downloaded,
        byte_len: 4 * START_MARGIN,
    };
    kernel::update::update(
        model,
        Message::Remote(RemoteEvent::Fetched {
            revision,
            result: Ok(fetched),
        }),
        Moment::default(),
    )
}

#[test]
fn a_lookahead_orders_one_prefetch_of_a_server_successor_and_preloads_it() {
    let (mut model, effects) = due();
    let mark = model.revisions.lookahead;

    let again = kernel::update::update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    );

    let prefetch_effects: Vec<&Effect> = effects
        .iter()
        .chain(&again)
        .flatten()
        .filter(|effect| matches!(effect, Effect::Remote(RemoteCmd::Prefetch(_))))
        .collect();
    let revision = prefetched(&model);
    assert_eq!(
        prefetch_effects,
        vec![&Effect::Remote(RemoteCmd::Prefetch(incoming(revision, 0)))]
    );
    assert!(matches!(
        &model.player,
        Player::Playing { preloaded: Some(track), .. } if *track == incoming_track()
    ));
}

#[test]
fn a_fetched_incoming_download_past_the_margin_preloads_it_growing() {
    let (mut model, _effects) = due();
    let revision = prefetched(&model);

    let effects = grown(&mut model, revision, START_MARGIN);

    assert_eq!(
        effects,
        Ok(vec![
            Effect::Audio(AudioCmd::Preload(TrackLoad {
                media: Media::Growing(GrowingMedia {
                    media_path: PathBuf::from("/cache/home/tr-1.part"),
                    downloaded: START_MARGIN,
                    byte_len: 4 * START_MARGIN,
                    revision,
                }),
                decibels: None,
                revision,
            })),
            Effect::Remote(RemoteCmd::Prefetch(incoming(revision, START_MARGIN))),
        ])
    );
}

#[test]
fn the_handover_makes_the_incoming_download_current() {
    let (mut model, _effects) = due();
    let revision = prefetched(&model);
    assert!(grown(&mut model, revision, START_MARGIN).is_ok());

    let handed = kernel::update::update(
        &mut model,
        Message::Audio(AudioEvent::TrackChanged),
        Moment::default(),
    );

    assert!(handed.iter().flatten().any(|effect| {
        *effect
            == Effect::Remote(RemoteCmd::Report {
                session: home_session(),
                play_report: PlayReport {
                    server_name: ServerName::new("home"),
                    server_track_id: ServerTrackId::new("tr-1"),
                    scrobble: Scrobble::NowPlaying,
                },
            })
    }));
    assert_eq!(
        grown(&mut model, revision, 2 * START_MARGIN),
        Ok(vec![
            Effect::Audio(AudioCmd::Grow {
                revision,
                downloaded: 2 * START_MARGIN,
            }),
            Effect::Remote(RemoteCmd::Fetch(incoming(revision, 2 * START_MARGIN))),
        ])
    );
}

#[test]
fn a_track_ending_before_its_successor_preloaded_starts_it_fresh() {
    let (mut model, _effects) = due();
    let revision = prefetched(&model);

    let ended = kernel::update::update(
        &mut model,
        Message::Audio(AudioEvent::Ended),
        Moment::default(),
    );

    assert_eq!(model.player, Player::Loading(incoming_track()));
    assert_eq!(model.downloads.len(), 1);
    let fresh = prefetched(&model);
    assert_ne!(fresh, revision);
    assert!(ended.iter().flatten().any(|effect| {
        *effect == Effect::Remote(RemoteCmd::Fetch(incoming(fresh, 0)))
    }));
}

#[test]
fn a_report_after_the_first_one_of_a_seek_sets_the_playhead_back() {
    let mut model = playing_model(3);
    sought(&mut model, secs(15), moment(1_000));
    for (reported, now) in [(secs(13), moment(2_000)), (secs(14), moment(3_000))] {
        assert!(
            update(
                &mut model,
                Message::Audio(AudioEvent::PositionReported {
                    position: reported,
                    revision: Revision::default().next(),
                }),
                now,
            )
            .is_ok()
        );
    }

    assert_eq!(model.player.position_at(moment(3_000)), secs(14));
}

#[test]
fn a_report_without_a_seek_sets_the_playhead_back() {
    let mut model = playing_model(3);
    assert!(update(&mut model, position(secs(20)), moment(1_000)).is_ok());

    assert!(update(&mut model, position(secs(12)), moment(2_000)).is_ok());

    assert_eq!(model.player.position_at(moment(2_000)), secs(12));
}
