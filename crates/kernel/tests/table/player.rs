use std::{sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, CoverJob, DiskCmd, Effect, LibraryCmd, MacosCmd, TrackLoad},
    domain::{
        cue::{Cue, PlaybackChange},
        geometry::Pixels,
        history::HistoryEntry,
        player::{PausedBy, Player},
        playhead::Playhead,
        revision::Revision,
        speed::Speed,
        time::Moment,
        track::Track,
    },
    message::{AudioError, DecodeError},
    update::{
        machine::Unhandled,
        player::{
            PlayerMessage,
            events::Lookahead,
            stamp::{Anchor, Stamp},
        },
    },
};
use rstest::rstest;

use crate::support::{table::cell, track_with_duration};

const COVER_SIDE: Pixels = Pixels(240);

const TRACK_LENGTH: Duration = Duration::from_secs(100);
const AT: Duration = Duration::from_secs(5);

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn now() -> Moment {
    Moment::new(Duration::from_secs(1_000))
}

fn speed() -> Speed {
    Speed::default()
}

fn anchor() -> Anchor {
    Anchor {
        started_at: now(),
        speed: speed(),
    }
}

fn later() -> Moment {
    Moment::new(Duration::from_secs(1_001))
}

fn anchor_later() -> Anchor {
    Anchor {
        started_at: later(),
        speed: speed(),
    }
}

fn revision() -> Revision {
    Revision::default().next()
}

fn stamp() -> Stamp {
    Stamp {
        anchor: anchor(),
        revision: revision(),
    }
}

fn head_at(position: Duration) -> Playhead {
    Playhead::anchored(position, now(), speed())
}

fn decode_error() -> AudioError {
    AudioError::Decode {
        path: "/tmp/a.flac".into(),
        error: DecodeError::Unsupported,
    }
}

fn error() -> PlayerMessage {
    PlayerMessage::Error(decode_error())
}

fn seek_error() -> PlayerMessage {
    PlayerMessage::Error(AudioError::Seek {
        diagnostic: kernel::domain::config::Diagnostic::from_error(
            &std::io::Error::other("the source cannot seek"),
        ),
    })
}

fn list_devices_error() -> PlayerMessage {
    PlayerMessage::Error(AudioError::ListDevices {
        diagnostic: kernel::domain::config::Diagnostic::from_error(
            &std::io::Error::other("the host cannot list its devices"),
        ),
    })
}

fn preload_error() -> PlayerMessage {
    PlayerMessage::Error(AudioError::Preload {
        path: "/tmp/b.flac".into(),
        error: DecodeError::Unsupported,
    })
}

fn lookahead_reached(
    position_secs: u64,
    ab_loop: Option<(u64, u64)>,
    next: Option<Arc<Track>>,
) -> PlayerMessage {
    PlayerMessage::LookaheadReached {
        position: secs(position_secs),
        lookahead: Lookahead {
            ab_loop: ab_loop
                .map(|(start_secs, end_secs)| (secs(start_secs), secs(end_secs))),
            next,
            duration: TRACK_LENGTH,
            now: now(),
            revision: revision(),
            cover_side: Some(COVER_SIDE),
        },
    }
}

fn track_changed(next: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::TrackChanged { next, now: now() }
}

fn ended(next: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::Ended {
        next,
        stamp: stamp(),
    }
}

fn now_playing(track: &Arc<Track>) -> Effect {
    Effect::Macos(MacosCmd::NowPlaying(Some(Arc::clone(track))))
}

fn appended_to_history(track: &Arc<Track>) -> Effect {
    Effect::Library(LibraryCmd::Disk(DiskCmd::AppendHistory(
        HistoryEntry::from_track(track, now()),
    )))
}

fn handed_off(track: &Arc<Track>, playback_change: PlaybackChange) -> Cmd {
    let mut effects = vec![appended_to_history(track), now_playing(track)];
    effects.extend(playback_change.effects());
    effects.push(Effect::Animate(Cue::TrackChanged));
    effects.push(Effect::Animate(Cue::PlaybackChanged(playback_change)));
    Cmd::from_iter(effects)
}

fn plays() -> Cmd {
    PlaybackChange::Play.cued()
}

fn pauses() -> Cmd {
    PlaybackChange::Pause.cued()
}

fn track_a() -> Arc<Track> {
    track_with_duration("/tmp/a.flac", TRACK_LENGTH)
}

fn track_b() -> Arc<Track> {
    track_with_duration("/tmp/b.flac", TRACK_LENGTH)
}

fn loading(track: Arc<Track>) -> Player {
    Player::Loading(track)
}

fn playing(
    track: Arc<Track>,
    position: Duration,
    preloaded: Option<Arc<Track>>,
) -> Player {
    Player::Playing {
        track,
        playhead: head_at(position),
        preloaded,
    }
}

fn reanchored(track: Arc<Track>, preloaded: Option<Arc<Track>>) -> Player {
    Player::Playing {
        track,
        playhead: Playhead::anchored(AT + secs(1), later(), speed()),
        preloaded,
    }
}

fn paused(track: Arc<Track>, position: Duration) -> Player {
    Player::Paused {
        track,
        position,
        by: PausedBy::Listener,
    }
}

fn held(track: Arc<Track>, position: Duration) -> Player {
    Player::Paused {
        track,
        position,
        by: PausedBy::Overlay,
    }
}

fn toggle(current: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::Toggle {
        current,
        stamp: stamp(),
    }
}

fn faded_in(track: &Arc<Track>) -> Cmd {
    let mut effects = vec![
        Effect::Audio(AudioCmd::Load(TrackLoad::for_track(track, revision()))),
        appended_to_history(track),
        now_playing(track),
    ];
    effects.extend(PlaybackChange::Play.effects());
    effects.push(Effect::Animate(Cue::TrackChanged));
    effects.push(Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Play)));
    Cmd::from_iter(effects)
}

fn cut_in(track: &Arc<Track>) -> Cmd {
    Cmd::effect(Effect::Audio(AudioCmd::Stop)).then(faded_in(track))
}

fn stopped() -> Cmd {
    PlaybackChange::Stop
        .cued()
        .then(Cmd::from(Effect::Macos(MacosCmd::NowPlaying(None))))
}

fn preloads(track: &Arc<Track>) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Preload(TrackLoad::for_track(track, revision()))),
        Effect::Library(LibraryCmd::PrefetchCover(CoverJob {
            path: track.path().to_path_buf(),
            side: COVER_SIDE,
        })),
    ])
}

fn seeks(seconds: u64) -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Seek(secs(seconds))),
        Effect::Macos(MacosCmd::SetPosition(secs(seconds))),
    ])
}

fn seek(target: u64) -> PlayerMessage {
    PlayerMessage::Seek {
        target: secs(target),
        now: now(),
    }
}

fn loaded(duration: Option<Duration>) -> PlayerMessage {
    PlayerMessage::Loaded {
        duration,
        anchor: anchor(),
    }
}

#[rstest]
#[case::stopped_toggle_starts_the_cursor_track(Player::Stopped, toggle(Some(track_a())), Ok((loading(track_a()), cut_in(&track_a()))))]
#[case::stopped_toggle_without_a_track_is_refused(
    Player::Stopped,
    toggle(None),
    Err(Unhandled)
)]
#[case::loading_toggle_is_refused(
    loading(track_a()),
    toggle(Some(track_a())),
    Err(Unhandled)
)]
#[case::playing_toggle_pauses_in_place(playing(track_a(), AT, Some(track_b())), toggle(Some(track_a())), Ok((paused(track_a(), AT), pauses())))]
#[case::paused_toggle_resumes_without_a_preload(paused(track_a(), AT), toggle(None), Ok((playing(track_a(), AT, None), plays())))]
#[case::stopped_stop_is_refused(Player::Stopped, PlayerMessage::Stop, Err(Unhandled))]
#[case::loading_stop_stops(loading(track_a()), PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::playing_stop_stops(playing(track_a(), AT, Some(track_b())), PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::paused_stop_stops(paused(track_a(), AT), PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::playing_hold_pauses_and_remembers_it(playing(track_a(), AT, Some(track_b())), PlayerMessage::Hold(now()), Ok((held(track_a(), AT), pauses())))]
#[case::playing_release_is_refused(
    playing(track_a(), AT, None),
    PlayerMessage::Release(anchor()),
    Err(Unhandled)
)]
#[case::held_release_resumes_without_a_preload(held(track_a(), AT), PlayerMessage::Release(anchor()), Ok((playing(track_a(), AT, None), plays())))]
#[case::held_toggle_resumes_and_ends_the_hold(held(track_a(), AT), toggle(None), Ok((playing(track_a(), AT, None), plays())))]
#[case::held_seek_keeps_the_hold(held(track_a(), AT), seek(7), Ok((held(track_a(), Duration::from_secs(7)), seeks(7))))]
#[case::held_track_changed_keeps_the_hold(held(track_a(), AT), track_changed(Some(track_b())), Ok((held(track_b(), Duration::ZERO), handed_off(&track_b(), PlaybackChange::Pause))))]
#[case::stopped_seek_is_refused(Player::Stopped, seek(7), Err(Unhandled))]
#[case::loading_seek_is_refused(loading(track_a()), seek(7), Err(Unhandled))]
#[case::playing_seek_moves_and_keeps_the_preload(playing(track_a(), AT, Some(track_b())), seek(7), Ok((playing(track_a(), Duration::from_secs(7), Some(track_b())), seeks(7))))]
#[case::paused_seek_moves(paused(track_a(), AT), seek(7), Ok((paused(track_a(), Duration::from_secs(7)), seeks(7))))]
#[case::playing_sleep_pauses(playing(track_a(), AT, None), PlayerMessage::SleepFired(now()), Ok((paused(track_a(), AT), pauses())))]
#[case::loading_sleep_is_refused(
    loading(track_a()),
    PlayerMessage::SleepFired(now()),
    Err(Unhandled)
)]
#[case::paused_sleep_is_refused(
    paused(track_a(), AT),
    PlayerMessage::SleepFired(now()),
    Err(Unhandled)
)]
#[case::stopped_sleep_is_refused(
    Player::Stopped,
    PlayerMessage::SleepFired(now()),
    Err(Unhandled)
)]
#[case::stopped_loaded_is_refused(Player::Stopped, loaded(None), Err(Unhandled))]
#[case::loading_loaded_plays_with_the_engines_duration(loading(track_a()), loaded(Some(secs(120))), Ok((playing(track_with_duration("/tmp/a.flac", secs(120)), Duration::ZERO, None), Cmd::none())))]
#[case::loading_loaded_without_a_duration_keeps_the_tags(loading(track_a()), loaded(None), Ok((playing(track_a(), Duration::ZERO, None), Cmd::none())))]
#[case::playing_loaded_is_refused(
    playing(track_a(), AT, None),
    loaded(None),
    Err(Unhandled)
)]
#[case::paused_loaded_is_refused(paused(track_a(), AT), loaded(None), Err(Unhandled))]
#[case::stopped_error_is_refused(Player::Stopped, error(), Err(Unhandled))]
#[case::loading_error_unwinds_the_start(loading(track_a()), error(), Ok((Player::Stopped, stopped())))]
#[case::playing_error_is_refused(
    playing(track_a(), AT, Some(track_b())),
    error(),
    Err(Unhandled)
)]
#[case::paused_error_is_refused(paused(track_a(), AT), error(), Err(Unhandled))]
#[case::loading_seek_error_is_refused(loading(track_a()), seek_error(), Err(Unhandled))]
#[case::playing_seek_error_is_refused(
    playing(track_a(), AT, None),
    seek_error(),
    Err(Unhandled)
)]
#[case::paused_seek_error_is_refused(
    paused(track_a(), AT),
    seek_error(),
    Err(Unhandled)
)]
#[case::stopped_seek_error_is_refused(Player::Stopped, seek_error(), Err(Unhandled))]
#[case::loading_list_devices_error_is_refused(
    loading(track_a()),
    list_devices_error(),
    Err(Unhandled)
)]
#[case::playing_list_devices_error_is_refused(
    playing(track_a(), AT, None),
    list_devices_error(),
    Err(Unhandled)
)]
#[case::loading_preload_error_is_refused(
    loading(track_a()),
    preload_error(),
    Err(Unhandled)
)]
#[case::playing_position_moves(playing(track_a(), AT, None), lookahead_reached(50, None, Some(track_b())), Ok((playing(track_a(), secs(50), None), Cmd::none())))]
#[case::playing_position_arms_the_preload_near_the_end(playing(track_a(), AT, None), lookahead_reached(95, None, Some(track_b())), Ok((playing(track_a(), secs(95), Some(track_b())), preloads(&track_b()))))]
#[case::playing_position_arms_nothing_when_nothing_follows(playing(track_a(), AT, None), lookahead_reached(95, None, None), Ok((playing(track_a(), secs(95), None), Cmd::none())))]
#[case::playing_position_arms_only_once(playing(track_a(), AT, Some(track_b())), lookahead_reached(95, None, Some(track_b())), Ok((playing(track_a(), secs(95), Some(track_b())), Cmd::none())))]
#[case::playing_position_past_the_loop_end_only_moves(playing(track_a(), AT, Some(track_b())), lookahead_reached(15, Some((5, 15)), Some(track_b())), Ok((playing(track_a(), secs(15), Some(track_b())), Cmd::none())))]
#[case::playing_speed_changed_moves_the_anchor_and_keeps_the_preload(playing(track_a(), AT, Some(track_b())), PlayerMessage::SpeedChanged(anchor_later()), Ok((reanchored(track_a(), Some(track_b())), Cmd::none())))]
#[case::playing_speed_changed_to_the_same_anchor_keeps_playing(playing(track_a(), AT, Some(track_b())), PlayerMessage::SpeedChanged(anchor()), Ok((playing(track_a(), AT, Some(track_b())), Cmd::none())))]
#[case::paused_speed_changed_is_refused(
    paused(track_a(), AT),
    PlayerMessage::SpeedChanged(anchor()),
    Err(Unhandled)
)]
#[case::stopped_speed_changed_is_refused(
    Player::Stopped,
    PlayerMessage::SpeedChanged(anchor()),
    Err(Unhandled)
)]
#[case::loading_speed_changed_is_refused(
    loading(track_a()),
    PlayerMessage::SpeedChanged(anchor()),
    Err(Unhandled)
)]
#[case::playing_output_lost_pauses(playing(track_a(), AT, Some(track_b())), PlayerMessage::OutputLost(now()), Ok((paused(track_a(), AT), pauses())))]
#[case::loading_output_lost_stops(loading(track_a()), PlayerMessage::OutputLost(now()), Ok((Player::Stopped, stopped())))]
#[case::paused_output_lost_is_refused(
    paused(track_a(), AT),
    PlayerMessage::OutputLost(now()),
    Err(Unhandled)
)]
#[case::stopped_output_lost_is_refused(
    Player::Stopped,
    PlayerMessage::OutputLost(now()),
    Err(Unhandled)
)]
#[case::stopped_track_changed_is_refused(
    Player::Stopped,
    track_changed(Some(track_b())),
    Err(Unhandled)
)]
#[case::loading_track_changed_is_refused(
    loading(track_a()),
    track_changed(Some(track_b())),
    Err(Unhandled)
)]
#[case::playing_track_changed_hands_off(playing(track_a(), AT, Some(track_b())), track_changed(Some(track_b())), Ok((playing(track_b(), Duration::ZERO, None), handed_off(&track_b(), PlaybackChange::Play))))]
#[case::playing_track_changed_without_a_next_restarts(playing(track_a(), AT, None), track_changed(None), Ok((playing(track_a(), Duration::ZERO, None), handed_off(&track_a(), PlaybackChange::Play))))]
#[case::paused_track_changed_keeps_the_pause(paused(track_a(), AT), track_changed(Some(track_b())), Ok((paused(track_b(), Duration::ZERO), handed_off(&track_b(), PlaybackChange::Pause))))]
#[case::playing_ended_crossfades_into_the_next(playing(track_a(), AT, None), ended(Some(track_b())), Ok((loading(track_b()), faded_in(&track_b()))))]
#[case::playing_ended_without_a_next_stops(playing(track_a(), AT, None), ended(None), Ok((Player::Stopped, stopped())))]
fn player_cell(
    #[case] player: Player,
    #[case] message: PlayerMessage,
    #[case] expected: Result<
        (Player, <Player as kernel::update::machine::Machine>::Effect),
        Unhandled,
    >,
) {
    cell(player, message, expected);
}

#[derive(Debug, Clone, Copy)]
enum RefusingPlayer {
    Stopped,
    Loading,
    Paused,
}

impl RefusingPlayer {
    fn player(self) -> Player {
        match self {
            RefusingPlayer::Stopped => Player::Stopped,
            RefusingPlayer::Loading => loading(track_a()),
            RefusingPlayer::Paused => paused(track_a(), AT),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RefusingMessage {
    Position,
    Ended,
    Hold,
    Release,
}

impl RefusingMessage {
    fn build(self) -> PlayerMessage {
        match self {
            RefusingMessage::Position => lookahead_reached(50, None, None),
            RefusingMessage::Ended => ended(Some(track_b())),
            RefusingMessage::Hold => PlayerMessage::Hold(now()),
            RefusingMessage::Release => PlayerMessage::Release(anchor()),
        }
    }
}

#[rstest]
fn message_is_refused_while_stopped_loading_or_paused(
    #[values(
        RefusingMessage::Position,
        RefusingMessage::Ended,
        RefusingMessage::Hold,
        RefusingMessage::Release
    )]
    message: RefusingMessage,
    #[values(
        RefusingPlayer::Stopped,
        RefusingPlayer::Loading,
        RefusingPlayer::Paused
    )]
    refusing_player: RefusingPlayer,
) {
    cell(refusing_player.player(), message.build(), Err(Unhandled));
}
