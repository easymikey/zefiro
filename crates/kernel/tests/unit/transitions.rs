use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, Media, TrackLoad, WindowColorsCmd},
    domain::{
        cursor::Cursor,
        direction::Direction,
        index::ViewIndex,
        model::Model,
        player::{AbLoop, PausedBy, Player},
        playhead::Playhead,
        playlist::RepeatMode,
        revision::Revision,
        speed::Speed,
        time::Moment,
        toast::ToastLevel,
        track::{AudioFormat, Tags, Track, TrackParts},
    },
    message::{BrowseRequest, LibraryEvent, Message, PlaybackRequest, Timer},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    effects,
    model_playing_at,
    model_with_tracks,
    track_at as arc_track,
    track_with_duration,
    update::{send, update},
};

fn driver_effects(cmd: Cmd) -> Vec<Effect> {
    effects(cmd)
        .into_iter()
        .filter(|effect| !matches!(effect, Effect::Animate(_)))
        .collect()
}

#[rstest]
#[case::playback_seek_by_negative_saturates_at_zero(
    Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Previous, by: Duration::from_secs(10) }),
    Duration::from_secs(3),
    Duration::ZERO
)]
#[case::playback_seek_by_positive_clamps_to_duration(
    Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Next, by: Duration::from_secs(10) }),
    Duration::from_secs(95),
    Duration::from_secs(100)
)]
#[case::playback_seek_to_clamps_to_duration(
    Message::Playback(PlaybackRequest::SeekTo(Duration::from_secs(500))),
    Duration::from_secs(10),
    Duration::from_secs(100)
)]
#[case::media_key_seek_forward_steps_ten_seconds(
    Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Next, by: Duration::from_secs(10) }),
    Duration::from_secs(30),
    Duration::from_secs(40)
)]
#[case::media_key_seek_back_steps_ten_seconds(
    Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Previous, by: Duration::from_secs(10) }),
    Duration::from_secs(40),
    Duration::from_secs(30)
)]
fn seek_routes_clamp_to_duration_regardless_of_message_source(
    #[case] message: Message,
    #[case] target: Duration,
    #[case] expected: Duration,
) {
    let mut model = Model {
        player: Player::Playing {
            track: Arc::new(Track::new(TrackParts {
                path: "/t.flac".into(),
                duration: Duration::from_secs(100),
                tags: Tags::default(),
                audio_format: AudioFormat::default(),
            })),
            playhead: Playhead::anchored(target, Moment::default(), Speed::default()),
            preloaded: None,
        },
        ..Default::default()
    };
    let cmd = update(&mut model, message, Moment::default()).unwrap();
    assert_eq!(model.player.position_at(Moment::default()), expected);
    let seek = effects(cmd);
    assert!(
        matches!(seek.first(), Some(Effect::Audio(AudioCmd::Seek { target: d, .. })) if *d == expected)
    );
    assert!(
        matches!(seek.get(1), Some(Effect::Macos(MacosCmd::SetPosition(d))) if *d == expected)
    );
}

#[test]
fn quit_stops_audio_flushes_config_and_reports_resets_the_window_colors_and_ends_with_quit()
 {
    let mut model = Model::default();
    let cmd = update(&mut model, Message::Quit, Moment::default()).unwrap();
    let (effects, _messages) = cmd.into_parts();
    assert!(matches!(
        effects.first(),
        Some(Effect::Audio(AudioCmd::Stop))
    ));
    assert!(matches!(
        effects.get(1),
        Some(Effect::Config(kernel::cmd::ConfigCmd::Flush))
    ));
    assert!(matches!(
        effects.get(2),
        Some(Effect::Remote(kernel::cmd::RemoteCmd::Flush(play_reports))) if play_reports.is_empty()
    ));
    assert!(matches!(
        effects.get(3),
        Some(Effect::WindowColors(WindowColorsCmd::Reset))
    ));
    assert!(matches!(effects.last(), Some(Effect::Quit)));
}

#[test]
fn jump_request_starts_selected_track() {
    let mut model = model_with_tracks(3);
    model.playlist.tracks[2] = Arc::new(Track::new(TrackParts {
        path: "/tmp/track2.flac".into(),
        duration: Duration::from_secs(42),
        tags: Tags::default(),
        audio_format: AudioFormat::default(),
    }));
    let cmd = update(
        &mut model,
        Message::Browse(BrowseRequest::JumpTo(ViewIndex::new(2))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(2)));
    assert!(matches!(model.player, Player::Loading(..)));
    let loading = model.player.current().unwrap();
    assert_eq!(loading.local_path(), Some(Path::new("/tmp/track2.flac")));
    assert!(effects(cmd).iter().any(
        |e| matches!(e, Effect::Audio(AudioCmd::Load(TrackLoad { media: Media::Local(p), .. })) if p == "/tmp/track2.flac")
    ));
}

#[test]
fn library_loaded_relists_the_playlist_without_effects() {
    let mut model = model_playing_at(3, 2, Duration::ZERO);
    let tracks = vec![arc_track("/new/a.flac"), arc_track("/new/b.flac")];
    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Loaded {
            tracks: tracks.clone(),
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(model.playlist.tracks, tracks);
    assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(1)));
    assert!(driver_effects(cmd).is_empty());
    assert!(model.player.is_playing());
}

#[rstest]
#[case::an_explicit_skip_snaps_it(2, Message::Playback(PlaybackRequest::Next), 1)]
#[case::a_natural_change_follows_it_from_the_playing_row(
    0,
    Message::Audio(kernel::message::AudioEvent::Ended),
    1
)]
#[case::a_natural_change_leaves_it_browsing_elsewhere(
    2,
    Message::Audio(kernel::message::AudioEvent::Ended),
    2
)]
fn a_track_change_moves_the_browse_cursor_only_when_it_should(
    #[case] cursor_index: usize,
    #[case] message: Message,
    #[case] expected: usize,
) {
    let mut model = model_playing_at(3, 0, Duration::ZERO);
    model.workspace.browse.cursor = Cursor::at(3, cursor_index);
    send(&mut model, message);
    assert_eq!(model.workspace.browse.selected(), ViewIndex::new(expected));
}

fn model_at_the_loop_end(player: impl FnOnce(Arc<Track>) -> Player) -> Model {
    let mut model = model_with_tracks(3);
    model.transport.ab_loop = Some(AbLoop::BothMarked {
        loop_start: Duration::from_secs(5),
        loop_end: Duration::from_secs(15),
    });
    model.player = player(track_with_duration(
        "/tmp/track0.flac",
        Duration::from_secs(100),
    ));
    model
}

fn playing_at(
    track: Arc<Track>,
    position: Duration,
    preloaded: Option<Arc<Track>>,
) -> Player {
    Player::Playing {
        track,
        playhead: Playhead::anchored(position, Moment::default(), Speed::default()),
        preloaded,
    }
}

#[rstest]
#[case::with_a_preload(Some(arc_track("/tmp/track1.flac")))]
#[case::without_a_preload(None)]
fn a_lookahead_at_the_loop_end_seeks_once_to_the_loop_start(
    #[case] preloaded: Option<Arc<Track>>,
) {
    let mut model = model_at_the_loop_end(|track| {
        playing_at(track, Duration::from_secs(15), preloaded.clone())
    });
    let mark = model.revisions.lookahead;
    let cmd = update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(
        driver_effects(cmd),
        vec![
            Effect::Audio(AudioCmd::Seek {
                target: Duration::from_secs(5),
                revision: Revision::default().next()
            }),
            Effect::Macos(MacosCmd::SetPosition(Duration::from_secs(5))),
            Effect::After {
                delay: Duration::from_secs(10),
                timer: Timer::Lookahead(model.revisions.lookahead),
            },
        ]
    );
    let track = track_with_duration("/tmp/track0.flac", Duration::from_secs(100));
    assert_eq!(
        model.player,
        playing_at(track, Duration::from_secs(5), preloaded)
    );
}

#[test]
fn a_lookahead_over_a_paused_player_at_the_loop_end_is_refused() {
    let mut model = model_at_the_loop_end(|track| Player::Paused {
        track,
        position: Duration::from_secs(15),
        by: PausedBy::Listener,
    });
    let before = model.clone();
    let mark = model.revisions.lookahead;
    let answer = update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    );
    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn track_ended_repeat_one_without_current_stops() {
    let mut model = model_playing_at(3, 0, Duration::ZERO);
    model.playlist.repeat_mode = RepeatMode::One;
    let cmd = update(
        &mut model,
        Message::Library(LibraryEvent::Loaded {
            tracks: vec![],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    assert!(model.player.is_playing());
    assert!(driver_effects(cmd).is_empty());

    let cmd2 = update(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::Ended),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(model.player, Player::Stopped);
    let effs = driver_effects(cmd2);
    insta::assert_debug_snapshot!(effs);
}

#[rstest]
#[case::a_load_that_never_arrives(
    Player::Loading(arc_track("/tmp/track0.flac")),
    kernel::message::AudioError::Decode {
        path: "/tmp/track0.flac".into(),
        error: kernel::message::DecodeError::Unreadable(kernel::domain::io_error::IoError::Missing),
    },
    "not found"
)]
#[case::a_preload_that_never_arrives(
    Player::Playing {
        track: arc_track("/tmp/track0.flac"),
        playhead: Playhead::anchored(Duration::from_secs(10), Moment::default(), Speed::default()),
        preloaded: Some(arc_track("/tmp/track1.flac")),
    },
    kernel::message::AudioError::OpenDevice { requested_device: kernel::domain::device::OutputDevice::SystemDefault, diagnostic: kernel::domain::config::Diagnostic::from_error(&std::io::Error::other("cannot preload /tmp/track1.flac: no such file")) },
    "cannot preload"
)]
#[case::a_seek_the_source_refuses(
    Player::Playing {
        track: arc_track("/tmp/track0.flac"),
        playhead: Playhead::anchored(Duration::from_secs(10), Moment::default(), Speed::default()),
        preloaded: None,
    },
    kernel::message::AudioError::Seek { diagnostic: kernel::domain::config::Diagnostic::from_error(&std::io::Error::other("the source cannot seek")) },
    "cannot seek"
)]
#[case::a_device_listing_failure_while_loading(
    Player::Loading(arc_track("/tmp/track0.flac")),
    kernel::message::AudioError::ListDevices { diagnostic: kernel::domain::config::Diagnostic::from_error(&std::io::Error::other("the host cannot list its devices")) },
    "cannot list"
)]
fn an_audio_failure_raises_an_error_toast(
    #[case] player: Player,
    #[case] error: kernel::message::AudioError,
    #[case] excerpt: &str,
) {
    let mut model = Model {
        player,
        ..Default::default()
    };
    send(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::Error(error)),
    );
    let toast = model.workspace.toasts.first().unwrap();
    assert_eq!(toast.level, ToastLevel::Error);
    let text = toast.text.as_deref().map_or("", str::trim);
    assert!(text.contains(excerpt), "got {text:?}");
}

#[test]
fn start_track_emits_nowplaying_and_playing_state() {
    let mut model = model_with_tracks(3);
    model.playlist.tracks[0] = Arc::new(Track::new(TrackParts {
        path: "/tmp/track0.flac".into(),
        duration: Duration::from_secs(200),
        tags: Tags {
            title: Some("Song".into()),
            artist: Some("Artist".into()),
            ..Tags::default()
        },
        audio_format: AudioFormat::default(),
    }));
    let cmd = update(
        &mut model,
        Message::Playback(PlaybackRequest::Toggle),
        Moment::default(),
    )
    .unwrap();
    let effs = effects(cmd);
    insta::assert_debug_snapshot!(effs);
    let now_playing = effs
        .iter()
        .find_map(|effect| match effect {
            Effect::Macos(MacosCmd::NowPlaying(shown)) => Some(shown.clone()),
            Effect::Macos(
                MacosCmd::SetPlayback(_)
                | MacosCmd::SetPosition(_)
                | MacosCmd::SetSpeed(_)
                | MacosCmd::SetVolume(_)
                | MacosCmd::Privacy,
            )
            | Effect::Audio(_)
            | Effect::Library(_)
            | Effect::Config(_)
            | Effect::Remote(_)
            | Effect::Animate(_)
            | Effect::RollShuffle(..)
            | Effect::WindowColors(_)
            | Effect::After { .. }
            | Effect::Restart(_)
            | Effect::Quit => None,
        })
        .unwrap();
    let shown = now_playing.expect("a track is shown");
    assert_eq!(shown.title(), "Song");
    assert_eq!(shown.tags().artist.as_deref(), Some("Artist"));
    assert_eq!(shown.tags().album, None);
    assert_eq!(shown.duration(), Some(Duration::from_secs(200)));
    assert_eq!(shown.local_path(), Some(Path::new("/tmp/track0.flac")));
}

#[test]
fn step_volume_emits_only_the_system_volume() {
    let mut model = Model::default();
    let cmd = update(
        &mut model,
        Message::Playback(PlaybackRequest::StepVolume(Direction::Next)),
        Moment::default(),
    )
    .unwrap();
    let effs = driver_effects(cmd);
    insta::assert_debug_snapshot!(effs);
    let [Effect::Macos(MacosCmd::SetVolume(system_volume))] = effs.as_slice() else {
        panic!("expected System(Volume) alone, got {effs:?}");
    };
    assert_eq!(*system_volume, model.transport.volume);
}
