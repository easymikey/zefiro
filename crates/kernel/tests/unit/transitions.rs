use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    AudioCmd,
    AudioFormat,
    Bounded,
    Cmd,
    ConfigEvent,
    Cue,
    Effect,
    LibraryEvent,
    MacosCmd,
    MacosEvent,
    Message,
    Model,
    Moment,
    Percent,
    PlaybackRequest,
    Player,
    Playhead,
    PlaylistRequest,
    Preload,
    Speed,
    Tags,
    Timer,
    ToastLevel,
    Track,
    TrackRequest,
    Transport,
    WindowColorsCmd,
    domain::{
        Action,
        Cursor,
        Direction,
        KeyOverride,
        KeymapOverrides,
        PlaylistIndex,
        Revision,
        ThemeName,
    },
    playlist::{PlayOrder, Playlist, RepeatMode},
    update::update,
};
use rstest::rstest;

use crate::support::{
    effects,
    model_playing_at,
    model_with_tracks,
    track_at as arc_track,
};

fn driver_effects(cmd: Cmd) -> Vec<Effect> {
    effects(cmd)
        .into_iter()
        .filter(|effect| !matches!(effect, Effect::Animate(_)))
        .collect()
}

fn ack_loaded(m: &mut Model) -> Cmd {
    update(
        m,
        Message::Audio(kernel::AudioEvent::Loaded { total: None }),
        Moment::default(),
    )
    .unwrap()
}

struct AdvanceFrom {
    start: usize,
    direction: Direction,
    repeat: RepeatMode,
}

#[rstest]
#[case::steps_forward(AdvanceFrom { start: 0, direction: Direction::Next, repeat: RepeatMode::Off }, true, PlaylistIndex::new(1))]
#[case::steps_backward(AdvanceFrom { start: 1, direction: Direction::Previous, repeat: RepeatMode::Off }, true, PlaylistIndex::new(0))]
#[case::stops_at_last_track_without_repeat(AdvanceFrom { start: 2, direction: Direction::Next, repeat: RepeatMode::Off }, false, PlaylistIndex::new(2))]
#[case::stops_at_first_track_without_repeat(AdvanceFrom { start: 0, direction: Direction::Previous, repeat: RepeatMode::Off }, false, PlaylistIndex::new(0))]
#[case::wraps_forward_with_repeat_all(AdvanceFrom { start: 2, direction: Direction::Next, repeat: RepeatMode::All }, true, PlaylistIndex::new(0))]
fn advance_respects_edges_and_repeat_all(
    #[case] from: AdvanceFrom,
    #[case] moved: bool,
    #[case] expected_index: PlaylistIndex,
) {
    let mut pl = load_three();
    pl.repeat = from.repeat;
    pl.cursor = Cursor::with_len(pl.tracks.len()).at(from.start);
    assert_eq!(pl.skip(from.direction).is_some(), moved);
    assert_eq!(pl.playing_index(), Some(expected_index));
}

fn load_three() -> Playlist {
    Playlist::from_tracks((0..3).map(|i| arc_track(&format!("/t{i}.flac"))).collect())
}

#[test]
fn prev_at_first_track_is_noop_without_repeat() {
    let mut m = model_with_tracks(3);
    let cmd = update(
        &mut m,
        Message::Playback(PlaybackRequest::Previous),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.playlist.playing_index(), Some(PlaylistIndex::new(0)));
    assert!(matches!(cmd, Cmd::None));
}

#[test]
fn volume_clamped_0_100() {
    let mut low = Model {
        transport: Transport {
            volume: Percent::clamped(3),
            ..Default::default()
        },
        ..Default::default()
    };
    let cmd = update(
        &mut low,
        Message::Playback(PlaybackRequest::NudgeVolume { steps: -5 }),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(low.transport.volume.get(), 0);
    let effs = effects(cmd);
    assert!(matches!(
        effs.as_slice(),
        [Effect::Macos(MacosCmd::Volume(system)), ..] if system.get() == 0
    ));

    let mut hi = Model {
        transport: Transport {
            volume: Percent::clamped(98),
            ..Default::default()
        },
        ..Default::default()
    };
    let _ = update(
        &mut hi,
        Message::Playback(PlaybackRequest::NudgeVolume { steps: 5 }),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(hi.transport.volume.get(), 100);
}

#[rstest]
#[case::playback_seek_by_negative_saturates_at_zero(
    Message::Playback(PlaybackRequest::SeekBy { seconds: -10 }),
    Duration::from_secs(3),
    Duration::ZERO
)]
#[case::playback_seek_by_positive_clamps_to_duration(
    Message::Playback(PlaybackRequest::SeekBy { seconds: 10 }),
    Duration::from_secs(95),
    Duration::from_secs(100)
)]
#[case::playback_seek_to_clamps_to_duration(
    Message::Playback(PlaybackRequest::SeekTo(Duration::from_secs(500))),
    Duration::from_secs(10),
    Duration::from_secs(100)
)]
#[case::media_key_seek_forward_steps_ten_seconds(
    Message::Playback(PlaybackRequest::SeekForward),
    Duration::from_secs(30),
    Duration::from_secs(40)
)]
#[case::media_key_seek_back_steps_ten_seconds(
    Message::Playback(PlaybackRequest::SeekBack),
    Duration::from_secs(40),
    Duration::from_secs(30)
)]
#[case::media_key_seek_back_saturates_at_zero(
    Message::Playback(PlaybackRequest::SeekBack),
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
    #[case] at: Duration,
    #[case] expected: Duration,
) {
    let mut m = Model {
        player: Player::Playing {
            track: Arc::new(
                Track::builder()
                    .path("/t.flac")
                    .duration(Duration::from_secs(100))
                    .tags(Tags::default())
                    .audio_format(AudioFormat::default())
                    .build(),
            ),
            head: Playhead::anchored(at, Moment::default(), Speed::default()),
            preload: Preload::None,
        },
        ..Default::default()
    };
    let cmd = update(&mut m, message, Moment::default()).unwrap();
    assert_eq!(m.player.position_at(Moment::default()), expected);
    let seek = effects(cmd);
    assert!(
        matches!(seek.first(), Some(Effect::Audio(AudioCmd::Seek(d))) if *d == expected)
    );
    assert!(
        matches!(seek.get(1), Some(Effect::Macos(MacosCmd::PlaybackPosition(d))) if *d == expected)
    );
}

#[test]
fn quit_stops_audio_resets_the_window_colors_and_ends_with_quit() {
    let mut m = Model::default();
    let cmd = update(&mut m, Message::Quit, Moment::default()).unwrap();
    let effects: Vec<Effect> = cmd.into_iter().collect();
    assert!(matches!(
        effects.first(),
        Some(Effect::Audio(AudioCmd::Stop))
    ));
    assert!(matches!(
        effects.get(1),
        Some(Effect::WindowColors(WindowColorsCmd::Reset))
    ));
    assert!(matches!(effects.last(), Some(Effect::Quit)));
}

#[test]
fn jump_request_starts_selected_track() {
    let mut m = model_with_tracks(3);
    m.playlist.tracks[2] = Arc::new(
        Track::builder()
            .path("/tmp/track2.flac")
            .duration(Duration::from_secs(42))
            .tags(Tags::default())
            .audio_format(AudioFormat::default())
            .build(),
    );
    let cmd = update(
        &mut m,
        Message::Playlist(PlaylistRequest::JumpTo(PlaylistIndex::new(2))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.playlist.playing_index(), Some(PlaylistIndex::new(2)));
    assert!(matches!(m.player, Player::Loading { .. }));
    let loading = m.player.current().unwrap();
    assert_eq!(loading.path(), Path::new("/tmp/track2.flac"));
    assert!(effects(cmd).iter().any(
        |e| matches!(e, Effect::Audio(AudioCmd::Load(TrackRequest { path: p, .. })) if p == "/tmp/track2.flac")
    ));

    let _ = ack_loaded(&mut m);
    assert!(m.player.is_playing());
    let playing = m.player.current().unwrap();
    assert_eq!(playing.duration(), Some(Duration::from_secs(42)));
    assert!(matches!(
        m.player,
        Player::Playing {
            preload: Preload::None,
            ..
        }
    ));
}

#[test]
fn jump_out_of_range_is_noop() {
    let mut m = model_with_tracks(3);
    let cmd = update(
        &mut m,
        Message::Playlist(PlaylistRequest::JumpTo(PlaylistIndex::new(9))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.playlist.playing_index(), Some(PlaylistIndex::new(0)));
    assert!(m.player.current().is_none());
    assert!(matches!(cmd, Cmd::None));
}

#[test]
fn library_loaded_relists_the_playlist_without_effects() {
    let mut m = model_playing_at(3, 2, Duration::ZERO);
    let tracks = vec![arc_track("/new/a.flac"), arc_track("/new/b.flac")];
    let cmd = update(
        &mut m,
        Message::Library(LibraryEvent::Loaded {
            tracks: tracks.clone(),
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.playlist.tracks, tracks);
    assert_eq!(m.playlist.playing_index(), Some(PlaylistIndex::new(1)));
    assert!(driver_effects(cmd).is_empty());
    assert!(m.player.is_playing());
}

#[test]
fn an_explicit_skip_snaps_the_browse_cursor() {
    let mut m = model_playing_at(3, 0, Duration::ZERO);
    m.workspace.browse.cursor = Cursor::with_len(3).at(2);
    let _ = update(
        &mut m,
        Message::Playback(PlaybackRequest::Next),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.workspace.browse.selected(), PlaylistIndex::new(1));
}

#[rstest]
#[case::following(0, 1)]
#[case::browsing_elsewhere(2, 2)]
fn a_natural_track_change_follows_only_a_cursor_that_was_on_the_playing_row(
    #[case] cursor: usize,
    #[case] expected: usize,
) {
    let mut m = model_playing_at(3, 0, Duration::ZERO);
    m.workspace.browse.cursor = Cursor::with_len(3).at(cursor);
    let _ = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::Ended),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.workspace.browse.selected(), PlaylistIndex::new(expected));
}

#[test]
fn toggling_shuffle_on_asks_for_an_order() {
    let mut m = model_with_tracks(4);
    let cmd = update(
        &mut m,
        Message::Playback(PlaybackRequest::ToggleShuffle),
        Moment::default(),
    )
    .unwrap();
    assert!(matches!(m.playlist.play_order, PlayOrder::ShufflePending));
    assert!(
        effects(cmd)
            .iter()
            .any(|effect| matches!(effect, Effect::RollShuffle { len: 4 }))
    );
}

#[test]
fn toggle_shuffle_off_clears_order() {
    let mut m = model_with_tracks(4);
    m.playlist.play_order = PlayOrder::Shuffle(vec![3, 1, 2, 0]);
    let cmd = update(
        &mut m,
        Message::Playback(PlaybackRequest::ToggleShuffle),
        Moment::default(),
    )
    .unwrap();
    assert!(matches!(m.playlist.play_order, PlayOrder::Linear));
    assert!(driver_effects(cmd).is_empty());
}

#[test]
fn preload_peeks_queue_head_when_queue_nonempty() {
    let mut m = model_with_tracks(3);
    m.player = Player::Playing {
        track: Arc::new(
            Track::builder()
                .path("/tmp/track0.flac")
                .duration(Duration::from_secs(100))
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        ),
        head: Playhead::anchored(Duration::ZERO, Moment::default(), Speed::default()),
        preload: Preload::None,
    };
    m.queue.push(PlaylistIndex::new(2));

    let _ = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::Playhead(Duration::from_secs(95))),
        Moment::default(),
    )
    .unwrap();
    let mark = m.revisions.mark;
    let cmd = update(
        &mut m,
        Message::Elapsed(Timer::Mark(mark)),
        Moment::default(),
    )
    .unwrap();
    assert!(cmd.effects().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Preload(TrackRequest { path, .. })) if path.as_os_str() == "/tmp/track2.flac"
    )));
}

#[test]
fn track_ended_repeat_one_without_current_stops() {
    let mut m = model_playing_at(3, 0, Duration::ZERO);
    m.playlist.repeat = RepeatMode::One;
    let cmd = update(
        &mut m,
        Message::Library(LibraryEvent::Loaded {
            tracks: vec![],
            revision: Revision::default(),
        }),
        Moment::default(),
    )
    .unwrap();
    assert!(m.player.is_playing());
    assert!(driver_effects(cmd).is_empty());

    let cmd2 = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::Ended),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.player, Player::Stopped);
    let effs = driver_effects(cmd2);
    insta::assert_debug_snapshot!(effs);
}

#[rstest]
#[case::a_load_that_never_arrives(
    Player::Loading {
        track: arc_track("/tmp/track0.flac"),
        at: Duration::ZERO,
    },
    kernel::AudioError::Decode {
        path: "/tmp/track0.flac".into(),
        kind: kernel::DecodeError::Unreadable(kernel::IoError::Missing),
    },
    "not found"
)]
#[case::a_preload_that_never_arrives(
    Player::Playing {
        track: arc_track("/tmp/track0.flac"),
        head: Playhead::anchored(Duration::from_secs(10), Moment::default(), Speed::default()),
        preload: Preload::Queued(arc_track("/tmp/track1.flac")),
    },
    kernel::AudioError::Stream {
        reason: "cannot preload /tmp/track1.flac: no such file".into(),
    },
    "cannot preload"
)]
#[case::a_seek_the_source_refuses(
    Player::Playing {
        track: arc_track("/tmp/track0.flac"),
        head: Playhead::anchored(Duration::from_secs(10), Moment::default(), Speed::default()),
        preload: Preload::None,
    },
    kernel::AudioError::Seek {
        reason: "the source cannot seek".into(),
    },
    "cannot seek"
)]
fn an_audio_failure_raises_an_error_toast(
    #[case] player: Player,
    #[case] error: kernel::AudioError,
    #[case] excerpt: &str,
) {
    let mut m = Model {
        player,
        ..Default::default()
    };
    let _ = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::Error(error)),
        Moment::default(),
    )
    .unwrap();
    let toast = m.workspace.toast.unwrap();
    assert_eq!(toast.level, ToastLevel::Error);
    assert!(toast.text.contains(excerpt), "got {:?}", toast.text);
}

#[test]
fn system_volume_sets_model_and_cues_without_an_audio_effect() {
    let mut m = Model::default();
    let cmd = update(
        &mut m,
        Message::Macos(MacosEvent::Volume(Percent::clamped(33))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.transport.volume.get(), 33);
    let raised: Vec<_> = cmd.effects().collect();
    assert!(matches!(
        raised.as_slice(),
        [Effect::Animate(Cue::VolumeChanged)]
    ));
}

#[test]
fn start_track_emits_nowplaying_and_playing_state() {
    let mut m = model_with_tracks(3);
    m.playlist.tracks[0] = Arc::new(
        Track::builder()
            .path("/tmp/track0.flac")
            .duration(Duration::from_secs(200))
            .tags(Tags {
                title: Some("Song".into()),
                artist: Some("Artist".into()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    );
    let cmd = update(
        &mut m,
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
                MacosCmd::PlaybackState(_)
                | MacosCmd::PlaybackPosition(_)
                | MacosCmd::Volume(_),
            )
            | Effect::Audio(_)
            | Effect::Library(_)
            | Effect::Config(_)
            | Effect::Animate(_)
            | Effect::RollShuffle { .. }
            | Effect::WindowColors(_)
            | Effect::After { .. }
            | Effect::Restart(_)
            | Effect::Quit => None,
        })
        .unwrap();
    let shown = now_playing.expect("a track is shown");
    assert_eq!(shown.song_title(), "Song");
    assert_eq!(shown.tags().artist.as_deref(), Some("Artist"));
    assert_eq!(shown.tags().album, None);
    assert_eq!(shown.duration(), Some(Duration::from_secs(200)));
    assert_eq!(shown.path(), Path::new("/tmp/track0.flac"));
}

#[test]
fn nudge_volume_emits_only_the_system_volume() {
    let mut m = Model::default();
    let cmd = update(
        &mut m,
        Message::Playback(PlaybackRequest::NudgeVolume { steps: 5 }),
        Moment::default(),
    )
    .unwrap();
    let effs = driver_effects(cmd);
    insta::assert_debug_snapshot!(effs);
    let [Effect::Macos(MacosCmd::Volume(system_volume))] = effs.as_slice() else {
        panic!("expected System(Volume) alone, got {effs:?}");
    };
    assert_eq!(*system_volume, m.transport.volume);
}

fn keymap_naming(chord: &str) -> KeymapOverrides {
    KeymapOverrides::from([(Action::PlayPause, KeyOverride::from(chord))])
}

#[rstest]
#[case::the_keymap_it_already_carries(KeymapOverrides::default())]
#[case::a_keymap_naming_another_chord(keymap_naming("space"))]
fn a_keymap_reload_moves_the_generation_only_when_the_file_says_something_new(
    #[case] keys: KeymapOverrides,
) {
    let mut model = Model::default();
    let before = model.revisions.config;

    let cmd = update(
        &mut model,
        Message::Config(ConfigEvent::KeymapReloaded(Box::new(keys.clone()))),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(
        model.revisions.config != before,
        keys != KeymapOverrides::default()
    );
    assert_eq!(model.workspace.keymap.overrides(), &keys);
    assert!(matches!(cmd, Cmd::None));
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
