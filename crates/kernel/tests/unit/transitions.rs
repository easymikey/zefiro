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
        player::Player,
        playhead::Playhead,
        playlist::{PlayOrder, Playlist, RepeatMode},
        revision::Revision,
        speed::Speed,
        theme::ThemeName,
        time::Moment,
        toast::ToastLevel,
        track::{AudioFormat, Tags, Track},
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
    update::{send, update},
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
        Message::Audio(kernel::message::AudioEvent::Loaded(None)),
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
#[case::steps_forward(AdvanceFrom { start: 0, direction: Direction::Next, repeat: RepeatMode::Off }, true, ViewIndex::new(1))]
#[case::steps_backward(AdvanceFrom { start: 1, direction: Direction::Previous, repeat: RepeatMode::Off }, true, ViewIndex::new(0))]
#[case::stops_at_last_track_without_repeat(AdvanceFrom { start: 2, direction: Direction::Next, repeat: RepeatMode::Off }, false, ViewIndex::new(2))]
#[case::stops_at_first_track_without_repeat(AdvanceFrom { start: 0, direction: Direction::Previous, repeat: RepeatMode::Off }, false, ViewIndex::new(0))]
#[case::wraps_forward_with_repeat_all(AdvanceFrom { start: 2, direction: Direction::Next, repeat: RepeatMode::All }, true, ViewIndex::new(0))]
fn advance_respects_edges_and_repeat_all(
    #[case] from: AdvanceFrom,
    #[case] moved: bool,
    #[case] expected_index: ViewIndex,
) {
    let mut pl = load_three();
    pl.repeat = from.repeat;
    pl.cursor = Cursor::at(pl.tracks.len(), from.start);
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
    assert_eq!(m.playlist.playing_index(), Some(ViewIndex::new(0)));
    assert!(cmd == Cmd::none());
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
        Message::Playback(PlaybackRequest::StepVolume(Direction::Previous)),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(low.transport.volume.get(), 0);
    let effs = effects(cmd);
    assert!(matches!(
        effs.as_slice(),
        [Effect::Macos(MacosCmd::SetVolume(system)), ..] if system.get() == 0
    ));

    let mut hi = Model {
        transport: Transport {
            volume: Percent::clamped(98),
            ..Default::default()
        },
        ..Default::default()
    };
    send(
        &mut hi,
        Message::Playback(PlaybackRequest::StepVolume(Direction::Next)),
    );
    assert_eq!(hi.transport.volume.get(), 100);
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
            playhead: Playhead::anchored(at, Moment::default(), Speed::default()),
            preloaded: None,
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
        matches!(seek.get(1), Some(Effect::Macos(MacosCmd::SetPosition(d))) if *d == expected)
    );
}

#[test]
fn quit_stops_audio_flushes_config_resets_the_window_colors_and_ends_with_quit() {
    let mut m = Model::default();
    let cmd = update(&mut m, Message::Quit, Moment::default()).unwrap();
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
        Message::Playback(PlaybackRequest::JumpTo(ViewIndex::new(2))),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.playlist.playing_index(), Some(ViewIndex::new(2)));
    assert!(matches!(m.player, Player::Loading(..)));
    let loading = m.player.current().unwrap();
    assert_eq!(loading.path(), Path::new("/tmp/track2.flac"));
    assert!(effects(cmd).iter().any(
        |e| matches!(e, Effect::Audio(AudioCmd::Load(TrackLoad { path: p, .. })) if p == "/tmp/track2.flac")
    ));

    drop(ack_loaded(&mut m));
    assert!(m.player.is_playing());
    let playing = m.player.current().unwrap();
    assert_eq!(playing.duration(), Some(Duration::from_secs(42)));
    assert!(matches!(
        m.player,
        Player::Playing {
            preloaded: None,
            ..
        }
    ));
}

#[test]
fn jump_out_of_range_is_refused() {
    let mut m = model_with_tracks(3);
    let result = update(
        &mut m,
        Message::Playback(PlaybackRequest::JumpTo(ViewIndex::new(9))),
        Moment::default(),
    );
    assert_eq!(m.playlist.playing_index(), Some(ViewIndex::new(0)));
    assert!(m.player.current().is_none());
    assert_eq!(result, Err(Unhandled));
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
    assert_eq!(m.playlist.playing_index(), Some(ViewIndex::new(1)));
    assert!(driver_effects(cmd).is_empty());
    assert!(m.player.is_playing());
}

#[test]
fn an_explicit_skip_snaps_the_browse_cursor() {
    let mut m = model_playing_at(3, 0, Duration::ZERO);
    m.workspace.browse.cursor = Cursor::at(3, 2);
    send(&mut m, Message::Playback(PlaybackRequest::Next));
    assert_eq!(m.workspace.browse.selected(), ViewIndex::new(1));
}

#[rstest]
#[case::following(0, 1)]
#[case::browsing_elsewhere(2, 2)]
fn a_natural_track_change_follows_only_a_cursor_that_was_on_the_playing_row(
    #[case] cursor: usize,
    #[case] expected: usize,
) {
    let mut m = model_playing_at(3, 0, Duration::ZERO);
    m.workspace.browse.cursor = Cursor::at(3, cursor);
    send(&mut m, Message::Audio(kernel::message::AudioEvent::Ended));
    assert_eq!(m.workspace.browse.selected(), ViewIndex::new(expected));
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
            .any(|effect| matches!(effect, Effect::RollShuffle(4)))
    );
}

#[test]
fn toggle_shuffle_off_clears_order() {
    let mut m = model_with_tracks(4);
    m.playlist.play_order =
        PlayOrder::Shuffle([3, 1, 2, 0].map(ViewIndex::new).to_vec());
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
        playhead: Playhead::anchored(
            Duration::ZERO,
            Moment::default(),
            Speed::default(),
        ),
        preloaded: None,
    };
    m.queue.push(m.playlist.tracks[2].source().clone());

    send(
        &mut m,
        Message::Audio(kernel::message::AudioEvent::Playhead(Duration::from_secs(
            95,
        ))),
    );
    let mark = m.revisions.lookahead;
    let cmd = update(
        &mut m,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    )
    .unwrap();
    assert!(cmd.effects().any(|effect| matches!(
        effect,
        Effect::Audio(AudioCmd::Preload(TrackLoad { path, .. })) if path.as_os_str() == "/tmp/track2.flac"
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
        Message::Audio(kernel::message::AudioEvent::Ended),
        Moment::default(),
    )
    .unwrap();
    assert_eq!(m.player, Player::Stopped);
    let effs = driver_effects(cmd2);
    insta::assert_debug_snapshot!(effs);
}

#[rstest]
#[case::a_load_that_never_arrives(
    Player::Loading(arc_track("/tmp/track0.flac")),
    kernel::message::AudioError::Decode {
        path: "/tmp/track0.flac".into(),
        kind: kernel::message::DecodeError::Unreadable(kernel::domain::io_error::IoError::Missing),
    },
    "not found"
)]
#[case::a_preload_that_never_arrives(
    Player::Playing {
        track: arc_track("/tmp/track0.flac"),
        playhead: Playhead::anchored(Duration::from_secs(10), Moment::default(), Speed::default()),
        preloaded: Some(arc_track("/tmp/track1.flac")),
    },
    kernel::message::AudioError::Stream { reason: kernel::domain::config::Diagnostic::from_error(&std::io::Error::other("cannot preload /tmp/track1.flac: no such file")) },
    "cannot preload"
)]
#[case::a_seek_the_source_refuses(
    Player::Playing {
        track: arc_track("/tmp/track0.flac"),
        playhead: Playhead::anchored(Duration::from_secs(10), Moment::default(), Speed::default()),
        preloaded: None,
    },
    kernel::message::AudioError::Seek { reason: kernel::domain::config::Diagnostic::from_error(&std::io::Error::other("the source cannot seek")) },
    "cannot seek"
)]
fn an_audio_failure_raises_an_error_toast(
    #[case] player: Player,
    #[case] error: kernel::message::AudioError,
    #[case] excerpt: &str,
) {
    let mut m = Model {
        player,
        ..Default::default()
    };
    send(
        &mut m,
        Message::Audio(kernel::message::AudioEvent::Error(error)),
    );
    let toast = m.workspace.toasts.first().unwrap();
    assert_eq!(toast.kind, ToastLevel::Error);
    let text = toast.text.as_deref().map_or("", str::trim);
    assert!(text.contains(excerpt), "got {text:?}");
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
                MacosCmd::SetPlayback(_)
                | MacosCmd::SetPosition(_)
                | MacosCmd::SetVolume(_),
            )
            | Effect::Audio(_)
            | Effect::Library(_)
            | Effect::Config(_)
            | Effect::Animate(_)
            | Effect::RollShuffle(..)
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
fn step_volume_emits_only_the_system_volume() {
    let mut m = Model::default();
    let cmd = update(
        &mut m,
        Message::Playback(PlaybackRequest::StepVolume(Direction::Next)),
        Moment::default(),
    )
    .unwrap();
    let effs = driver_effects(cmd);
    insta::assert_debug_snapshot!(effs);
    let [Effect::Macos(MacosCmd::SetVolume(system_volume))] = effs.as_slice() else {
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
    assert!(cmd == Cmd::none());
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
