use std::{sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, CoverJob, Effect, LibraryCmd, MacosCmd, TrackLoad},
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
        since: now(),
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

fn head_at(at: Duration) -> Playhead {
    Playhead::anchored(at, now(), speed())
}

fn decode_error() -> AudioError {
    AudioError::Decode {
        path: "/tmp/a.flac".into(),
        kind: DecodeError::Unsupported,
    }
}

fn error() -> PlayerMessage {
    PlayerMessage::Error {
        error: decode_error(),
        now: now(),
    }
}

fn seek_error() -> PlayerMessage {
    PlayerMessage::Error {
        error: AudioError::Seek {
            reason: kernel::domain::config::Diagnostic::from_error(
                &std::io::Error::other("the source cannot seek"),
            ),
        },
        now: now(),
    }
}

fn tick(at: u64, ab: Option<(u64, u64)>, next: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::LookaheadReached {
        offset: secs(at),
        lookahead: Lookahead {
            ab_loop: ab.map(|(a, b)| (secs(a), secs(b))),
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

fn next(track: Arc<Track>) -> PlayerMessage {
    PlayerMessage::Start {
        track,
        stamp: stamp(),
    }
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
    Effect::Library(LibraryCmd::AppendHistory(HistoryEntry::from_track(
        track,
        now(),
    )))
}

fn handed_off(track: &Arc<Track>, playback: PlaybackChange) -> Cmd {
    let mut effects = vec![appended_to_history(track), now_playing(track)];
    effects.extend(playback.effects());
    effects.push(Effect::Animate(Cue::TrackChanged));
    effects.push(Effect::Animate(Cue::PlaybackChanged(playback)));
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

fn playing(track: Arc<Track>, at: Duration, preloaded: Option<Arc<Track>>) -> Player {
    Player::Playing {
        track,
        playhead: head_at(at),
        preloaded,
    }
}

fn paused(track: Arc<Track>, at: Duration) -> Player {
    Player::Paused {
        track,
        position: at,
        by: PausedBy::Listener,
    }
}

fn held(track: Arc<Track>, at: Duration) -> Player {
    Player::Paused {
        track,
        position: at,
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

fn loaded(total: Option<Duration>) -> PlayerMessage {
    PlayerMessage::Loaded {
        total,
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
#[case::stopped_stop_stops_again(Player::Stopped, PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
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
#[case::playing_seek_moves_and_downgrades_the_preload(playing(track_a(), AT, Some(track_b())), seek(7), Ok((playing(track_a(), Duration::from_secs(7), Some(track_b())), seeks(7))))]
#[case::paused_seek_moves(paused(track_a(), AT), seek(7), Ok((paused(track_a(), Duration::from_secs(7)), seeks(7))))]
#[case::playing_sleep_pauses(playing(track_a(), AT, None), PlayerMessage::SleepFired(now()), Ok((paused(track_a(), AT), pauses())))]
#[case::stopped_next_starts(Player::Stopped, next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::loading_next_interrupts_the_load(loading(track_a()), next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::playing_next_drops_the_preload_and_starts(playing(track_a(), AT, Some(track_a())), next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::paused_next_starts(paused(track_a(), AT), next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::stopped_loaded_is_refused(Player::Stopped, loaded(None), Err(Unhandled))]
#[case::loading_loaded_plays_with_the_engines_duration(loading(track_a()), loaded(Some(secs(120))), Ok((playing(track_with_duration("/tmp/a.flac", secs(120)), Duration::ZERO, None), Cmd::none())))]
#[case::loading_loaded_without_a_duration_keeps_the_tags(loading(track_a()), loaded(None), Ok((playing(track_a(), Duration::ZERO, None), Cmd::none())))]
#[case::playing_loaded_is_refused(
    playing(track_a(), AT, None),
    loaded(None),
    Err(Unhandled)
)]
#[case::paused_loaded_is_refused(paused(track_a(), AT), loaded(None), Err(Unhandled))]
#[case::stopped_error_is_the_no_device_report(Player::Stopped, error(), Err(Unhandled))]
#[case::loading_error_unwinds_the_start(loading(track_a()), error(), Ok((Player::Stopped, stopped())))]
#[case::playing_error_keeps_playing(
    playing(track_a(), AT, Some(track_b())),
    error(),
    Err(Unhandled)
)]
#[case::paused_error_keeps_the_pause(paused(track_a(), AT), error(), Err(Unhandled))]
#[case::loading_seek_error_keeps_the_load(
    loading(track_a()),
    seek_error(),
    Err(Unhandled)
)]
#[case::playing_seek_error_keeps_playing(
    playing(track_a(), AT, None),
    seek_error(),
    Err(Unhandled)
)]
#[case::playing_position_moves(playing(track_a(), AT, None), tick(50, None, Some(track_b())), Ok((playing(track_a(), secs(50), None), Cmd::none())))]
#[case::playing_position_arms_the_preload_near_the_end(playing(track_a(), AT, None), tick(95, None, Some(track_b())), Ok((playing(track_a(), secs(95), Some(track_b())), preloads(&track_b()))))]
#[case::playing_position_arms_nothing_when_nothing_follows(playing(track_a(), AT, None), tick(95, None, None), Ok((playing(track_a(), secs(95), None), Cmd::none())))]
#[case::playing_position_arms_only_once(playing(track_a(), AT, Some(track_b())), tick(95, None, Some(track_b())), Ok((playing(track_a(), secs(95), Some(track_b())), Cmd::none())))]
#[case::playing_position_closes_the_loop(playing(track_a(), AT, Some(track_b())), tick(15, Some((5, 15)), Some(track_b())), Ok((playing(track_a(), secs(5), Some(track_b())), seeks(5))))]
#[case::playing_reanchor_keeps_the_position_and_the_preload(playing(track_a(), AT, Some(track_b())), PlayerMessage::Reanchor(anchor()), Ok((playing(track_a(), AT, Some(track_b())), Cmd::none())))]
#[case::paused_reanchor_changes_nothing(paused(track_a(), AT), PlayerMessage::Reanchor(anchor()), Ok((paused(track_a(), AT), Cmd::none())))]
#[case::stopped_reanchor_changes_nothing(Player::Stopped, PlayerMessage::Reanchor(anchor()), Ok((Player::Stopped, Cmd::none())))]
#[case::loading_reanchor_changes_nothing(loading(track_a()), PlayerMessage::Reanchor(anchor()), Ok((loading(track_a()), Cmd::none())))]
#[case::playing_position_past_the_loop_without_a_preload_seeks(playing(track_a(), AT, None), tick(15, Some((5, 15)), Some(track_b())), Ok((playing(track_a(), secs(5), None), seeks(5))))]
#[case::paused_position_closes_the_loop(paused(track_a(), AT), tick(15, Some((5, 15)), None), Ok((paused(track_a(), secs(5)), seeks(5))))]
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
    #[case] start: Player,
    #[case] message: PlayerMessage,
    #[case] expected: Result<
        (Player, <Player as kernel::update::machine::Machine>::Effect),
        Unhandled,
    >,
) {
    cell(start, message, expected);
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
    SleepFired,
}

impl RefusingMessage {
    fn build(self) -> PlayerMessage {
        match self {
            RefusingMessage::Position => tick(50, None, None),
            RefusingMessage::Ended => ended(Some(track_b())),
            RefusingMessage::Hold => PlayerMessage::Hold(now()),
            RefusingMessage::Release => PlayerMessage::Release(anchor()),
            RefusingMessage::SleepFired => PlayerMessage::SleepFired(now()),
        }
    }
}

#[rstest]
fn message_is_refused_while_stopped_loading_or_paused(
    #[values(
        RefusingMessage::Position,
        RefusingMessage::Ended,
        RefusingMessage::Hold,
        RefusingMessage::Release,
        RefusingMessage::SleepFired
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
