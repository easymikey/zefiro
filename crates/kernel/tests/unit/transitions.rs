use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd, TrackLoad, WindowColorsCmd},
    domain::{
        bounded::Bounded,
        cue::Cue,
        cursor::Cursor,
        direction::Direction,
        index::ViewIndex,
        keymap::{Action, KeyOverride, KeymapOverrides},
        model::Model,
        percent::Percent,
        player::{AbLoop, PausedBy, Player},
        playhead::Playhead,
        playlist::{PlayOrder, Playlist, RepeatMode},
        revision::Revision,
        speed::Speed,
        theme::ThemeName,
        time::Moment,
        toast::ToastLevel,
        track::{AudioFormat, Tags, Track, TrackParts},
        transport::Transport,
    },
    message::{ConfigEvent, LibraryEvent, MacosEvent, Message, PlaybackRequest, Timer},
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

fn ack_loaded(model: &mut Model) -> Cmd {
    update(
        model,
        Message::Audio(kernel::message::AudioEvent::Loaded(None)),
        Moment::default(),
    )
    .unwrap()
}

struct AdvanceFrom {
    start: usize,
    direction: Direction,
    repeat_mode: RepeatMode,
}

#[rstest]
#[case::steps_forward(AdvanceFrom { start: 0, direction: Direction::Next, repeat_mode: RepeatMode::Off }, true, ViewIndex::new(1))]
#[case::steps_backward(AdvanceFrom { start: 1, direction: Direction::Previous, repeat_mode: RepeatMode::Off }, true, ViewIndex::new(0))]
#[case::stops_at_last_track_without_repeat(AdvanceFrom { start: 2, direction: Direction::Next, repeat_mode: RepeatMode::Off }, false, ViewIndex::new(2))]
#[case::stops_at_first_track_without_repeat(AdvanceFrom { start: 0, direction: Direction::Previous, repeat_mode: RepeatMode::Off }, false, ViewIndex::new(0))]
#[case::wraps_forward_with_repeat_all(AdvanceFrom { start: 2, direction: Direction::Next, repeat_mode: RepeatMode::All }, true, ViewIndex::new(0))]
fn advance_respects_edges_and_repeat_all(
    #[case] from: AdvanceFrom,
    #[case] moved: bool,
    #[case] expected_index: ViewIndex,
) {
    let mut pl = load_three();
    pl.repeat_mode = from.repeat_mode;
    pl.cursor = Cursor::at(pl.tracks.len(), from.start);
    assert_eq!(pl.skip(from.direction).is_some(), moved);
    assert_eq!(pl.playing_index(), Some(expected_index));
}

fn load_three() -> Playlist {
    Playlist::from_tracks((0..3).map(|i| arc_track(&format!("/t{i}.flac"))).collect())
}

#[test]
fn prev_at_first_track_is_refused_without_repeat() {
    let mut model = model_with_tracks(3);
    let result = update(
        &mut model,
        Message::Playback(PlaybackRequest::Previous),
        Moment::default(),
    );
    assert_eq!(result, Err(Unhandled));
    assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(0)));
}

#[test]
fn volume_clamped_0_100() {
    {
        let mut model = Model {
            transport: Transport {
                volume: Percent::clamped(3),
                ..Default::default()
            },
            ..Default::default()
        };
        let cmd = update(
            &mut model,
            Message::Playback(PlaybackRequest::StepVolume(Direction::Previous)),
            Moment::default(),
        )
        .unwrap();
        assert_eq!(model.transport.volume.get(), 0);
        let effs = effects(cmd);
        assert!(matches!(
            effs.as_slice(),
            [Effect::Macos(MacosCmd::SetVolume(system)), ..] if system.get() == 0
        ));
    }

    {
        let mut model = Model {
            transport: Transport {
                volume: Percent::clamped(98),
                ..Default::default()
            },
            ..Default::default()
        };
        send(
            &mut model,
            Message::Playback(PlaybackRequest::StepVolume(Direction::Next)),
        );
        assert_eq!(model.transport.volume.get(), 100);
    }
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
#[case::media_key_seek_back_saturates_at_zero(
    Message::Playback(PlaybackRequest::SeekBy { direction: Direction::Previous, by: Duration::from_secs(10) }),
    Duration::from_secs(3),
    Duration::ZERO
)]
#[case::media_key_seek_to_clamps_to_duration(
    Message::Playback(PlaybackRequest::SeekTo(Duration::from_secs(500))),
    Duration::from_secs(10),
    Duration::from_secs(100)
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
        matches!(seek.first(), Some(Effect::Audio(AudioCmd::Seek(d))) if *d == expected)
    );
    assert!(
        matches!(seek.get(1), Some(Effect::Macos(MacosCmd::SetPosition(d))) if *d == expected)
    );
}

#[test]
fn quit_stops_audio_flushes_config_resets_the_window_colors_and_ends_with_quit() {
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
        Message::Playback(PlaybackRequest::JumpTo(ViewIndex::new(2))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(2)));
    assert!(matches!(model.player, Player::Loading(..)));
    let loading = model.player.current().unwrap();
    assert_eq!(loading.local_path(), Some(Path::new("/tmp/track2.flac")));
    assert!(effects(cmd).iter().any(
        |e| matches!(e, Effect::Audio(AudioCmd::Load(TrackLoad { path: p, .. })) if p == "/tmp/track2.flac")
    ));

    drop(ack_loaded(&mut model));
    assert!(model.player.is_playing());
    let playing = model.player.current().unwrap();
    assert_eq!(playing.duration(), Some(Duration::from_secs(42)));
    assert!(matches!(
        model.player,
        Player::Playing {
            preloaded: None,
            ..
        }
    ));
}

#[test]
fn jump_out_of_range_is_refused() {
    let mut model = model_with_tracks(3);
    let result = update(
        &mut model,
        Message::Playback(PlaybackRequest::JumpTo(ViewIndex::new(9))),
        Moment::default(),
    );
    assert_eq!(model.playlist.playing_index(), Some(ViewIndex::new(0)));
    assert!(model.player.current().is_none());
    assert_eq!(result, Err(Unhandled));
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

#[test]
fn an_explicit_skip_snaps_the_browse_cursor() {
    let mut model = model_playing_at(3, 0, Duration::ZERO);
    model.workspace.browse.cursor = Cursor::at(3, 2);
    send(&mut model, Message::Playback(PlaybackRequest::Next));
    assert_eq!(model.workspace.browse.selected(), ViewIndex::new(1));
}

#[rstest]
#[case::following(0, 1)]
#[case::browsing_elsewhere(2, 2)]
fn a_natural_track_change_follows_only_a_cursor_that_was_on_the_playing_row(
    #[case] cursor_index: usize,
    #[case] expected: usize,
) {
    let mut model = model_playing_at(3, 0, Duration::ZERO);
    model.workspace.browse.cursor = Cursor::at(3, cursor_index);
    send(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::Ended),
    );
    assert_eq!(model.workspace.browse.selected(), ViewIndex::new(expected));
}

#[test]
fn toggling_shuffle_on_asks_for_an_order() {
    let mut model = model_with_tracks(4);
    let cmd = update(
        &mut model,
        Message::Playback(PlaybackRequest::ToggleShuffle),
        Moment::default(),
    )
    .unwrap();
    assert!(matches!(
        model.playlist.play_order,
        PlayOrder::ShufflePending
    ));
    assert!(
        effects(cmd)
            .iter()
            .any(|effect| matches!(effect, Effect::RollShuffle(4)))
    );
}

#[test]
fn toggle_shuffle_off_clears_order() {
    let mut model = model_with_tracks(4);
    model.playlist.play_order =
        PlayOrder::Shuffled([3, 1, 2, 0].map(ViewIndex::new).to_vec());
    let cmd = update(
        &mut model,
        Message::Playback(PlaybackRequest::ToggleShuffle),
        Moment::default(),
    )
    .unwrap();
    assert!(matches!(model.playlist.play_order, PlayOrder::Linear));
    assert!(driver_effects(cmd).is_empty());
}

#[test]
fn preload_peeks_queue_head_when_queue_nonempty() {
    let mut model = model_with_tracks(3);
    model.player = Player::Playing {
        track: Arc::new(Track::new(TrackParts {
            path: "/tmp/track0.flac".into(),
            duration: Duration::from_secs(100),
            tags: Tags::default(),
            audio_format: AudioFormat::default(),
        })),
        playhead: Playhead::anchored(
            Duration::ZERO,
            Moment::default(),
            Speed::default(),
        ),
        preloaded: None,
    };
    model.queue.push(model.playlist.tracks[2].source().clone());

    send(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::PositionReported(
            Duration::from_secs(95),
        )),
    );
    let mark = model.revisions.lookahead;
    let cmd = update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    )
    .unwrap();
    assert!(cmd.effects().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Preload(TrackLoad { path, .. })) if path.as_os_str() == "/tmp/track2.flac"
    )));
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
            Effect::Audio(AudioCmd::Seek(Duration::from_secs(5))),
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
fn a_device_listing_failure_while_loading_keeps_the_load() {
    let player = Player::Loading(arc_track("/tmp/track0.flac"));
    let mut model = Model {
        player: player.clone(),
        ..Default::default()
    };
    send(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::Error(
            kernel::message::AudioError::ListDevices {
                diagnostic: kernel::domain::config::Diagnostic::from_error(
                    &std::io::Error::other("the host cannot list its devices"),
                ),
            },
        )),
    );
    assert_eq!(model.player, player);
    let toast = model.workspace.toasts.first().unwrap();
    assert_eq!(toast.level, ToastLevel::Error);
}

#[test]
fn system_volume_sets_model_and_cues_without_an_audio_effect() {
    let mut model = Model::default();
    let cmd = update(
        &mut model,
        Message::Macos(MacosEvent::VolumeChanged(Percent::clamped(33))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(model.transport.volume.get(), 33);
    let raised: Vec<_> = cmd.effects().collect();
    assert!(matches!(
        raised.as_slice(),
        [Effect::Animate(Cue::VolumeChanged)]
    ));
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
                | MacosCmd::SetVolume(_),
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

fn keymap_naming(chord: &str) -> KeymapOverrides {
    KeymapOverrides::from([(Action::PlayPause, KeyOverride::from(chord))])
}

#[rstest]
#[case::the_keymap_it_already_carries(KeymapOverrides::default(), Err(Unhandled))]
#[case::a_keymap_naming_another_chord(keymap_naming("space"), Ok(Cmd::none()))]
fn a_keymap_reload_moves_the_generation_only_when_the_file_says_something_new(
    #[case] keymap_overrides: KeymapOverrides,
    #[case] expected: Result<Cmd, Unhandled>,
) {
    let mut model = Model::default();
    let before = model.revisions.config;

    let result = update(
        &mut model,
        Message::Config(ConfigEvent::KeymapReloaded(Box::new(
            keymap_overrides.clone(),
        ))),
        Moment::default(),
    );

    assert_eq!(
        model.revisions.config != before,
        keymap_overrides != KeymapOverrides::default()
    );
    assert_eq!(model.workspace.keymap.overrides(), &keymap_overrides);
    assert_eq!(result, expected);
}

#[test]
fn a_theme_reload_bumps_the_theme_generation_and_raises_a_cue() {
    let mut model = Model::default();
    let before = model.revisions.theme;

    let cmd = update(
        &mut model,
        Message::Config(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir"))),
        Moment::default(),
    )
    .unwrap();

    assert_ne!(model.revisions.theme, before);
    assert!(
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Animate(Cue::ThemeChanged)))
    );
}
