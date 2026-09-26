use std::{sync::Arc, time::Duration};

use kernel::{
    AudioCmd,
    AudioFailure,
    Bounded,
    Cmd,
    Cue,
    DecodeFault,
    Effect,
    LibraryCmd,
    Moment,
    NowPlaying,
    Pause,
    Percent,
    PlaybackChange,
    Player,
    Playhead,
    Preload,
    Speed,
    SystemCmd,
    Track,
    domain::{Revision, UnixSeconds},
    update::player::{Anchor, Lookahead, PlayerMessage, PlayerRejection, Resume},
};
use rstest::rstest;

use crate::support::{table::cell, track_with_duration};

const TRACK_LENGTH: Duration = Duration::from_secs(100);
const AT: Duration = Duration::from_secs(5);
const PRELOAD_LEAD: Duration = Duration::from_secs(10);

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
        now: now(),
        speed: speed(),
    }
}

fn head_at(at: Duration) -> Playhead {
    Playhead::anchored(at, now(), speed())
}

fn fault() -> AudioFailure {
    AudioFailure::Decode {
        path: "/tmp/a.flac".into(),
        fault: DecodeFault::Unsupported,
    }
}

fn error() -> PlayerMessage {
    PlayerMessage::Error {
        failure: fault(),
        now: now(),
    }
}

fn seek_error() -> PlayerMessage {
    PlayerMessage::Error {
        failure: AudioFailure::Seek {
            reason: "the source cannot seek".to_string(),
        },
        now: now(),
    }
}

fn tick(at: u64, ab: Option<(u64, u64)>, next: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::Playhead {
        offset: secs(at),
        lookahead: Lookahead {
            preload_lead: PRELOAD_LEAD,
            ab_loop: ab.map(|(a, b)| (secs(a), secs(b))),
            next,
            duration: TRACK_LENGTH,
            now: now(),
        },
    }
}

fn track_changed(next: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::TrackChanged { next, now: now() }
}

fn next(track: Arc<Track>) -> PlayerMessage {
    PlayerMessage::Start {
        track,
        volume: volume(),
    }
}

fn ended(next: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::Ended {
        next,
        volume: volume(),
    }
}

fn now_playing(track: &Arc<Track>) -> Effect {
    Effect::System(SystemCmd::NowPlaying(NowPlaying::Track {
        title: track.song_title(),
        artist: track.tags().artist.clone(),
        album: track.tags().album.clone(),
        duration: track.duration().unwrap_or_default(),
        path: track.path().to_path_buf(),
    }))
}

fn appended_to_history(track: &Arc<Track>) -> Effect {
    Effect::Library(LibraryCmd::AppendHistory {
        track: Arc::clone(track),
        at: UnixSeconds::UNSTAMPED,
    })
}

fn handed_off(track: &Arc<Track>, playback: PlaybackChange) -> Cmd {
    let mut effects = vec![appended_to_history(track), now_playing(track)];
    effects.extend(playback.effects());
    effects.push(Effect::Animate(Cue::TrackChanged));
    effects.push(Effect::Animate(Cue::PlaybackChanged(playback)));
    Cmd::Batch(effects)
}

fn plays() -> Cmd {
    PlaybackChange::Play.cued()
}

fn pauses() -> Cmd {
    PlaybackChange::Pause.cued()
}

fn volume() -> Percent {
    Percent::clamped(70)
}

fn track_a() -> Arc<Track> {
    track_with_duration("/tmp/a.flac", TRACK_LENGTH)
}

fn track_b() -> Arc<Track> {
    track_with_duration("/tmp/b.flac", TRACK_LENGTH)
}

fn loading(track: Arc<Track>) -> Player {
    Player::Loading {
        track,
        at: Duration::ZERO,
    }
}

fn playing(track: Arc<Track>, at: Duration, preload: Preload) -> Player {
    Player::Playing {
        track,
        head: head_at(at),
        preload,
    }
}

fn paused(track: Arc<Track>, at: Duration) -> Player {
    Player::Paused {
        track,
        at,
        pause: Pause::ByListener,
    }
}

fn held(track: Arc<Track>, at: Duration) -> Player {
    Player::Paused {
        track,
        at,
        pause: Pause::ByOverlay,
    }
}

fn toggle(current: Option<Arc<Track>>) -> PlayerMessage {
    PlayerMessage::Toggle {
        current,
        resume: Resume {
            volume: volume(),
            anchor: anchor(),
        },
    }
}

fn faded_in(track: &Arc<Track>) -> Cmd {
    let mut effects = vec![
        Effect::Audio(AudioCmd::Load {
            path: track.path().to_path_buf(),
            gain: track.audio_format().replay_gain,
            revision: Revision::UNSTAMPED,
        }),
        Effect::Audio(AudioCmd::Volume(volume())),
        appended_to_history(track),
        now_playing(track),
    ];
    effects.extend(PlaybackChange::Play.effects());
    effects.push(Effect::Animate(Cue::TrackChanged));
    effects.push(Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Play)));
    Cmd::Batch(effects)
}

fn cut_in(track: &Arc<Track>) -> Cmd {
    Cmd::One(Effect::Audio(AudioCmd::Stop)).then(faded_in(track))
}

fn stopped() -> Cmd {
    let mut effects = PlaybackChange::Stop.effects().to_vec();
    effects.push(Effect::System(SystemCmd::NowPlaying(NowPlaying::Cleared)));
    effects.push(Effect::Animate(Cue::PlaybackChanged(PlaybackChange::Stop)));
    Cmd::Batch(effects)
}

fn preloads(track: &Arc<Track>) -> Cmd {
    Cmd::Batch(vec![
        Effect::Audio(AudioCmd::Preload {
            path: track.path().to_path_buf(),
            gain: track.audio_format().replay_gain,
            revision: Revision::UNSTAMPED,
        }),
        Effect::Library(LibraryCmd::PrefetchCover(track.path().to_path_buf())),
    ])
}

fn seeks(seconds: u64) -> Cmd {
    Cmd::Batch(vec![
        Effect::Audio(AudioCmd::Seek(secs(seconds))),
        Effect::System(SystemCmd::PlaybackPosition(secs(seconds))),
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

type Cell = crate::support::table::Cell<Player>;

#[rstest]
#[case::stopped_toggle_starts_the_cursor_track(Player::Stopped, toggle(Some(track_a())), Ok((loading(track_a()), cut_in(&track_a()))))]
#[case::stopped_toggle_without_a_track_is_refused(
    Player::Stopped,
    toggle(None),
    Err(PlayerRejection::Stopped)
)]
#[case::loading_toggle_is_refused(
    loading(track_a()),
    toggle(Some(track_a())),
    Err(PlayerRejection::Loading)
)]
#[case::playing_toggle_pauses_in_place(playing(track_a(), AT, Preload::Queued(track_b())), toggle(Some(track_a())), Ok((paused(track_a(), AT), pauses())))]
#[case::paused_toggle_resumes_without_a_preload(paused(track_a(), AT), toggle(None), Ok((playing(track_a(), AT, Preload::None), plays())))]
#[case::stopped_stop_stops_again(Player::Stopped, PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::loading_stop_stops(loading(track_a()), PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::playing_stop_stops(playing(track_a(), AT, Preload::Queued(track_b())), PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::paused_stop_stops(paused(track_a(), AT), PlayerMessage::Stop, Ok((Player::Stopped, stopped())))]
#[case::playing_hold_pauses_and_remembers_it(playing(track_a(), AT, Preload::Queued(track_b())), PlayerMessage::Hold(now()), Ok((held(track_a(), AT), pauses())))]
#[case::playing_release_changes_nothing(playing(track_a(), AT, Preload::None), PlayerMessage::Release(anchor()), Ok((playing(track_a(), AT, Preload::None), Cmd::None)))]
#[case::held_release_resumes_without_a_preload(held(track_a(), AT), PlayerMessage::Release(anchor()), Ok((playing(track_a(), AT, Preload::None), plays())))]
#[case::held_toggle_resumes_and_ends_the_hold(held(track_a(), AT), toggle(None), Ok((playing(track_a(), AT, Preload::None), plays())))]
#[case::held_seek_keeps_the_hold(held(track_a(), AT), seek(7), Ok((held(track_a(), Duration::from_secs(7)), seeks(7))))]
#[case::held_track_changed_keeps_the_hold(held(track_a(), AT), track_changed(Some(track_b())), Ok((held(track_b(), Duration::ZERO), handed_off(&track_b(), PlaybackChange::Pause))))]
#[case::stopped_seek_is_refused(
    Player::Stopped,
    seek(7),
    Err(PlayerRejection::Stopped)
)]
#[case::loading_seek_is_refused(
    loading(track_a()),
    seek(7),
    Err(PlayerRejection::Loading)
)]
#[case::playing_seek_moves_and_downgrades_the_preload(playing(track_a(), AT, Preload::Queued(track_b())), seek(7), Ok((playing(track_a(), Duration::from_secs(7), Preload::Stale(track_b())), seeks(7))))]
#[case::paused_seek_moves(paused(track_a(), AT), seek(7), Ok((paused(track_a(), Duration::from_secs(7)), seeks(7))))]
#[case::playing_sleep_pauses(playing(track_a(), AT, Preload::None), PlayerMessage::SleepFired(now()), Ok((paused(track_a(), AT), pauses())))]
#[case::stopped_next_starts(Player::Stopped, next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::loading_next_interrupts_the_load(loading(track_a()), next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::playing_next_drops_the_preload_and_starts(playing(track_a(), AT, Preload::Queued(track_a())), next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::paused_next_starts(paused(track_a(), AT), next(track_b()), Ok((loading(track_b()), cut_in(&track_b()))))]
#[case::stopped_loaded_is_refused(
    Player::Stopped,
    loaded(None),
    Err(PlayerRejection::Stopped)
)]
#[case::loading_loaded_plays_with_the_engines_duration(loading(track_a()), loaded(Some(secs(120))), Ok((playing(track_with_duration("/tmp/a.flac", secs(120)), Duration::ZERO, Preload::None), Cmd::None)))]
#[case::loading_loaded_without_a_duration_keeps_the_tags(loading(track_a()), loaded(None), Ok((playing(track_a(), Duration::ZERO, Preload::None), Cmd::None)))]
#[case::playing_loaded_is_refused(
    playing(track_a(), AT, Preload::None),
    loaded(None),
    Err(PlayerRejection::Playing)
)]
#[case::paused_loaded_is_refused(
    paused(track_a(), AT),
    loaded(None),
    Err(PlayerRejection::Paused)
)]
#[case::stopped_error_is_the_no_device_report(Player::Stopped, error(), Ok((Player::Stopped, Cmd::None)))]
#[case::loading_error_unwinds_the_start(loading(track_a()), error(), Ok((Player::Stopped, stopped())))]
#[case::playing_error_keeps_playing(playing(track_a(), AT, Preload::Queued(track_b())), error(), Ok((playing(track_a(), AT, Preload::Queued(track_b())), Cmd::None)))]
#[case::paused_error_keeps_the_pause(paused(track_a(), AT), error(), Ok((paused(track_a(), AT), Cmd::None)))]
#[case::loading_seek_error_keeps_the_load(loading(track_a()), seek_error(), Ok((loading(track_a()), Cmd::None)))]
#[case::playing_seek_error_keeps_playing(playing(track_a(), AT, Preload::None), seek_error(), Ok((playing(track_a(), AT, Preload::None), Cmd::None)))]
#[case::playing_position_moves(playing(track_a(), AT, Preload::None), tick(50, None, Some(track_b())), Ok((playing(track_a(), secs(50), Preload::None), Cmd::None)))]
#[case::playing_position_arms_the_preload_near_the_end(playing(track_a(), AT, Preload::None), tick(95, None, Some(track_b())), Ok((playing(track_a(), secs(95), Preload::Queued(track_b())), preloads(&track_b()))))]
#[case::playing_position_arms_nothing_when_nothing_follows(playing(track_a(), AT, Preload::None), tick(95, None, None), Ok((playing(track_a(), secs(95), Preload::None), Cmd::None)))]
#[case::playing_position_arms_only_once(playing(track_a(), AT, Preload::Stale(track_b())), tick(95, None, Some(track_b())), Ok((playing(track_a(), secs(95), Preload::Stale(track_b())), Cmd::None)))]
#[case::playing_position_closes_the_loop(playing(track_a(), AT, Preload::Queued(track_b())), tick(15, Some((5, 15)), Some(track_b())), Ok((playing(track_a(), secs(5), Preload::Stale(track_b())), seeks(5))))]
#[case::paused_position_closes_the_loop(paused(track_a(), AT), tick(15, Some((5, 15)), None), Ok((paused(track_a(), secs(5)), seeks(5))))]
#[case::stopped_track_changed_is_refused(
    Player::Stopped,
    track_changed(Some(track_b())),
    Err(PlayerRejection::Stopped)
)]
#[case::loading_track_changed_is_refused(
    loading(track_a()),
    track_changed(Some(track_b())),
    Err(PlayerRejection::Loading)
)]
#[case::playing_track_changed_hands_off(playing(track_a(), AT, Preload::Queued(track_b())), track_changed(Some(track_b())), Ok((playing(track_b(), Duration::ZERO, Preload::None), handed_off(&track_b(), PlaybackChange::Play))))]
#[case::playing_track_changed_without_a_next_restarts(playing(track_a(), AT, Preload::None), track_changed(None), Ok((playing(track_a(), Duration::ZERO, Preload::None), handed_off(&track_a(), PlaybackChange::Play))))]
#[case::paused_track_changed_keeps_the_pause(paused(track_a(), AT), track_changed(Some(track_b())), Ok((paused(track_b(), Duration::ZERO), handed_off(&track_b(), PlaybackChange::Pause))))]
#[case::playing_ended_crossfades_into_the_next(playing(track_a(), AT, Preload::None), ended(Some(track_b())), Ok((loading(track_b()), faded_in(&track_b()))))]
#[case::playing_ended_without_a_next_stops(playing(track_a(), AT, Preload::None), ended(None), Ok((Player::Stopped, stopped())))]
fn player_cell(
    #[case] start: Player,
    #[case] message: PlayerMessage,
    #[case] expected: Cell,
) {
    cell(start, message, expected);
}

#[derive(Debug, Clone, Copy)]
enum RefusingState {
    Stopped,
    Loading,
    Paused,
}

impl RefusingState {
    fn player(self) -> Player {
        match self {
            RefusingState::Stopped => Player::Stopped,
            RefusingState::Loading => loading(track_a()),
            RefusingState::Paused => paused(track_a(), AT),
        }
    }

    fn reason(self) -> PlayerRejection {
        match self {
            RefusingState::Stopped => PlayerRejection::Stopped,
            RefusingState::Loading => PlayerRejection::Loading,
            RefusingState::Paused => PlayerRejection::Paused,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RefusingMessage {
    Position,
    Ended,
}

impl RefusingMessage {
    fn build(self) -> PlayerMessage {
        match self {
            RefusingMessage::Position => tick(50, None, None),
            RefusingMessage::Ended => ended(Some(track_b())),
        }
    }
}

#[rstest]
fn message_is_refused_while_stopped_loading_or_paused(
    #[values(RefusingMessage::Position, RefusingMessage::Ended)]
    message: RefusingMessage,
    #[values(RefusingState::Stopped, RefusingState::Loading, RefusingState::Paused)]
    state: RefusingState,
) {
    cell(state.player(), message.build(), Err(state.reason()));
}

#[derive(Debug, Clone, Copy)]
enum IdleMessage {
    Hold,
    Release,
    SleepFired,
}

impl IdleMessage {
    fn build(self) -> PlayerMessage {
        match self {
            IdleMessage::Hold => PlayerMessage::Hold(now()),
            IdleMessage::Release => PlayerMessage::Release(anchor()),
            IdleMessage::SleepFired => PlayerMessage::SleepFired(now()),
        }
    }
}

#[rstest]
fn message_changes_nothing_while_stopped_loading_or_paused(
    #[values(IdleMessage::Hold, IdleMessage::Release, IdleMessage::SleepFired)]
    message: IdleMessage,
    #[values(RefusingState::Stopped, RefusingState::Loading, RefusingState::Paused)]
    state: RefusingState,
) {
    cell(
        state.player(),
        message.build(),
        Ok((state.player(), Cmd::None)),
    );
}
