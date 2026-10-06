use std::time::Duration;

use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        bounded::Bounded,
        cue::{Cue, PlaybackChange},
        direction::Direction,
        index::ViewIndex,
        model::Model,
        overlay::OverlayName,
        percent::Percent,
        revision::Revision,
        theme::ThemeName,
        time::Moment,
        toast::Toast,
    },
    message::{
        AudioEvent,
        BrowseRequest,
        ConfigEvent,
        LibraryEvent,
        MacosEvent,
        Message,
        OverlayRequest,
        PlaybackRequest,
        QueueRequest,
        Timer,
    },
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::{
    model_with_tracks,
    playing_model,
    router::moon_library_scanned,
    update::update,
};

fn cues(model: &mut Model, messages: Vec<Message>) -> Vec<Cue> {
    let mut seen = Vec::new();
    for message in messages {
        let cmd = update(model, message, Moment::default()).unwrap();
        seen.extend(found(&cmd));
    }
    seen
}

fn found(cmd: &Cmd) -> Vec<Cue> {
    cmd.effects()
        .filter_map(|effect| match effect {
            Effect::Animate(cue) => Some(*cue),
            Effect::Audio(_)
            | Effect::Library(_)
            | Effect::Macos(_)
            | Effect::Config(_)
            | Effect::WindowColors(_)
            | Effect::RollShuffle(..)
            | Effect::After { .. }
            | Effect::Restart(_)
            | Effect::Quit => None,
        })
        .collect()
}

fn open(name: OverlayName) -> Message {
    Message::Overlay(OverlayRequest::Open(name))
}

fn toasted() -> Message {
    Message::Toast(Toast::error("boom".to_string()))
}

#[rstest]
#[case::opening_an_overlay_raises_a_cue(
    model_with_tracks(3),
    vec![open(OverlayName::Help)],
    Cue::OverlayOpened
)]
#[case::closing_an_overlay_raises_a_cue(
    model_with_tracks(3),
    vec![open(OverlayName::Help), Message::Overlay(OverlayRequest::Close)],
    Cue::OverlayClosed
)]
#[case::a_toast_raises_a_cue(model_with_tracks(3), vec![toasted()], Cue::ToastRaised)]
#[case::the_next_key_dismisses_the_toast_with_a_cue(
    model_with_tracks(3),
    vec![toasted(), open(OverlayName::Help)],
    Cue::ToastDismissed
)]
#[case::starting_a_track_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Playback(PlaybackRequest::Toggle)],
    Cue::TrackChanged
)]
#[case::pausing_raises_a_playback_cue(
    playing_model(3),
    vec![Message::Playback(PlaybackRequest::Toggle)],
    Cue::PlaybackChanged(PlaybackChange::Pause)
)]
#[case::queuing_a_track_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Queue(QueueRequest::ToggleAt(ViewIndex::new(1)))],
    Cue::QueueChanged
)]
#[case::favoriting_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Browse(BrowseRequest::ToggleFavorite)],
    Cue::FavoriteToggled
)]
#[case::toggling_shuffle_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Playback(PlaybackRequest::ToggleShuffle)],
    Cue::PlayOrderChanged
)]
#[case::cycling_repeat_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Playback(PlaybackRequest::CycleRepeat)],
    Cue::PlayOrderChanged
)]
#[case::stepping_the_volume_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Playback(PlaybackRequest::StepVolume(Direction::Next))],
    Cue::VolumeChanged
)]
#[case::the_system_raising_the_volume_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Macos(MacosEvent::VolumeChanged(Percent::clamped(60)))],
    Cue::VolumeChanged
)]
#[case::trashing_a_track_raises_a_cue(
    moon_library_scanned(),
    vec![open(OverlayName::ConfirmTrash), Message::Overlay(OverlayRequest::Confirm)],
    Cue::TrackTrashed
)]
#[case::reloading_the_theme_raises_a_cue(
    model_with_tracks(3),
    vec![Message::Config(ConfigEvent::ThemeReloaded(ThemeName::from_static("noir")))],
    Cue::ThemeChanged
)]
#[case::the_library_landing_raises_a_cue(
    Model::default(),
    vec![Message::Library(LibraryEvent::Loaded {
        tracks: Vec::new(),
        revision: Revision::default(),
    })],
    Cue::LibraryOpened
)]
fn a_transition_raises_its_cue(
    #[case] mut model: Model,
    #[case] messages: Vec<Message>,
    #[case] expected: Cue,
) {
    let seen = cues(&mut model, messages);
    assert!(
        seen.contains(&expected),
        "expected {expected:?} among {seen:?}"
    );
}

#[test]
fn the_system_echoing_a_volume_is_refused() {
    let mut model = model_with_tracks(3);
    let message = Message::Macos(MacosEvent::VolumeChanged(model.transport.volume));

    let result = update(&mut model, message, Moment::default());

    assert_eq!(result, Err(Unhandled));
}

#[rstest]
#[case::a_toast_timer_without_a_toast_stays_silent(
    model_with_tracks(3),
    vec![Message::Elapsed(Timer::Toast(Revision::default()))]
)]
fn a_non_transition_stays_silent(
    #[case] mut model: Model,
    #[case] messages: Vec<Message>,
) {
    let seen = cues(&mut model, messages);
    assert!(seen.is_empty(), "expected no cue, saw {seen:?}");
}

#[rstest]
#[case::the_next_key(vec![
    Message::Playback(PlaybackRequest::Next),
    Message::Audio(AudioEvent::Loaded(None)),
    Message::Audio(AudioEvent::PositionReported(Duration::from_millis(100))),
])]
#[case::a_gapless_handoff(vec![
    Message::Audio(AudioEvent::PositionReported(Duration::from_millis(100))),
    Message::Audio(AudioEvent::TrackChanged),
    Message::Audio(AudioEvent::PositionReported(Duration::from_millis(10))),
])]
#[case::the_track_running_out(vec![
    Message::Audio(AudioEvent::Ended),
    Message::Audio(AudioEvent::Loaded(None)),
    Message::Audio(AudioEvent::PositionReported(Duration::from_millis(10))),
])]
fn one_track_change_cues_one_sweep_and_one_chip_pulse(#[case] messages: Vec<Message>) {
    let seen = cues(&mut playing_model(3), messages);

    assert_eq!(
        seen,
        vec![
            Cue::TrackChanged,
            Cue::PlaybackChanged(PlaybackChange::Play)
        ]
    );
}

#[test]
fn favoriting_twice_raises_two_cues_where_the_diff_saw_none() {
    let mut model = model_with_tracks(3);
    let seen = cues(
        &mut model,
        vec![
            Message::Browse(BrowseRequest::ToggleFavorite),
            Message::Browse(BrowseRequest::ToggleFavorite),
        ],
    );
    assert_eq!(
        seen.iter()
            .filter(|cue| **cue == Cue::FavoriteToggled)
            .count(),
        2,
        "both toggles must raise a cue, saw {seen:?}"
    );
}

#[test]
fn enqueuing_the_same_track_twice_empties_the_queue_and_raises_a_cue_each_time() {
    let mut model = model_with_tracks(3);
    let queued_message = Message::Queue(QueueRequest::ToggleAt(ViewIndex::new(1)));
    let seen = cues(&mut model, vec![queued_message.clone(), queued_message]);

    assert!(model.queue.is_empty());
    assert_eq!(seen, vec![Cue::QueueChanged, Cue::QueueChanged]);
}
