use std::time::Duration;

use kernel::{
    AbLoop,
    AudioEvent,
    Bounded,
    Cmd,
    Message,
    Model,
    Moment,
    Nudge,
    OverlayName,
    PlaybackRequest,
    Timer,
    domain::{Cursor, Overlay, Player, PlaylistIndex, Revision, Transport},
    playlist::{PlayOrder, RepeatMode},
    update::{
        Rejection,
        overlay::{HistoryRejection, OverlayRejection},
        player::PlayerRejection,
        update,
    },
};
use rstest::rstest;

use crate::support::{
    model_with_tracks,
    router::{
        a_lap_of,
        acknowledged,
        close,
        confirm,
        ended,
        enqueue,
        handed_off,
        history_enqueue,
        jump_char,
        logged,
        mark_ab,
        mark_fires,
        media,
        moon_library,
        moon_library_scanned,
        moon_library_selecting,
        near_the_end,
        nudge_speed,
        open,
        playing_nothing_selected,
        queued,
        repeating,
        search_backspace,
        search_enqueue,
        search_moon,
        search_nav,
        shuffle,
        shuffled,
        skip,
        spinning,
        spinning_at,
        spinning_past,
        text_char,
        toasted,
        typed,
    },
};

type Step = (
    Message,
    Cmd,
    Player,
    Option<Overlay>,
    Cursor,
    Cursor,
    Vec<PlaylistIndex>,
    RepeatMode,
    PlayOrder,
    Transport,
    Option<kernel::Toast>,
);

fn walked(mut model: Model, messages: Vec<Message>) -> Vec<Step> {
    messages
        .into_iter()
        .map(|message| {
            let message = resolved(message, &model);
            let sent = message.clone();
            let cmd = update(&mut model, message, Moment::default()).unwrap();
            (
                sent,
                cmd,
                model.player.clone(),
                model.workspace.overlay.clone(),
                model.workspace.browse.cursor,
                model.playlist.at,
                model.queue.clone(),
                model.playlist.repeat,
                model.playlist.play_order.clone(),
                model.transport.clone(),
                model.workspace.toast.clone(),
            )
        })
        .collect()
}

fn resolved(message: Message, model: &Model) -> Message {
    if let Message::Elapsed(Timer::Mark(Revision::UNSTAMPED)) = message {
        return Message::Elapsed(Timer::Mark(model.mark_generation));
    }
    message
}

#[rstest]
#[case::search_typing_reranks_and_backspace_widens_it_back(
    "search_typing_reranks_and_backspace_widens_it_back",
    moon_library(),
    search_moon(vec![search_backspace(), search_backspace(), search_backspace(), search_backspace()])
)]
#[case::search_confirm_plays_the_track_the_selected_match_resolves_to(
    "search_confirm_plays_the_track_the_selected_match_resolves_to",
    moon_library(),
    search_moon(vec![search_nav(Nudge::Down), confirm()])
)]
#[case::search_enqueue_queues_the_resolved_track_and_stays_open(
    "search_enqueue_queues_the_resolved_track_and_stays_open",
    moon_library(),
    search_moon(vec![search_nav(Nudge::Down), search_enqueue()])
)]
#[case::search_esc_leaves_the_browse_cursor_where_it_was(
    "search_esc_leaves_the_browse_cursor_where_it_was",
    moon_library_selecting(2),
    search_moon(vec![search_nav(Nudge::Down), close()])
)]
#[case::help_opens_over_the_browse_cursor_and_closes_off_it(
    "help_opens_over_the_browse_cursor_and_closes_off_it",
    moon_library_selecting(2),
    vec![open(OverlayName::Help), close()]
)]
#[case::confirm_delete_captures_the_selected_track_and_trashes_it(
    "confirm_delete_captures_the_selected_track_and_trashes_it",
    moon_library_scanned(),
    vec![open(OverlayName::ConfirmDelete), confirm()]
)]
#[case::confirm_delete_cancelled_trashes_nothing(
    "confirm_delete_cancelled_trashes_nothing",
    moon_library_scanned(),
    vec![open(OverlayName::ConfirmDelete), close()]
)]
#[case::track_details_shows_the_selected_playlist_track(
    "track_details_shows_the_selected_playlist_track",
    moon_library_selecting(1),
    vec![open(OverlayName::TrackDetails), close()]
)]
#[case::track_details_falls_back_to_the_playing_track(
    "track_details_falls_back_to_the_playing_track",
    playing_nothing_selected(Duration::from_secs(100)),
    vec![open(OverlayName::TrackDetails), close()]
)]
#[case::opening_history_loads_the_log(
    "opening_history_loads_the_log",
    logged(&["/m/a.flac"], &["/m/a.flac"]),
    vec![open(OverlayName::History), close()]
)]
#[case::history_enqueue_queues_the_library_track_at_the_selected_path(
    "history_enqueue_queues_the_library_track_at_the_selected_path",
    logged(&["/m/a.flac", "/m/b.flac"], &["/m/a.flac", "/m/b.flac"]),
    vec![history_enqueue()]
)]
#[case::history_enqueue_of_a_path_no_longer_in_the_library_says_so(
    "history_enqueue_of_a_path_no_longer_in_the_library_says_so",
    logged(&["/m/gone.flac"], &[]),
    vec![history_enqueue()]
)]
#[case::jump_confirm_clamps_the_parsed_target_to_the_track(
    "jump_confirm_clamps_the_parsed_target_to_the_track",
    playing_nothing_selected(Duration::from_secs(100)),
    {
        let mut messages = vec![open(OverlayName::JumpToTime)];
        messages.extend(typed("10:00", jump_char));
        messages.push(confirm());
        messages
    }
)]
#[case::saving_a_playlist_confirms_the_validated_name(
    "saving_a_playlist_confirms_the_validated_name",
    model_with_tracks(1),
    {
        let mut messages = vec![open(OverlayName::SavePlaylist)];
        messages.extend(typed("mix", text_char));
        messages.push(confirm());
        messages
    }
)]
#[case::saving_a_playlist_under_a_rejected_name_stays_open_with_the_error(
    "saving_a_playlist_under_a_rejected_name_stays_open_with_the_error",
    model_with_tracks(1),
    {
        let mut messages = vec![open(OverlayName::SavePlaylist)];
        messages.extend(typed("...", text_char));
        messages.push(confirm());
        messages
    }
)]
#[case::settings_closes_like_every_other_overlay(
    "settings_closes_like_every_other_overlay",
    Model::default(),
    vec![open(OverlayName::Settings), close()]
)]
#[case::a_gapless_cycle_advances_without_a_load(
    "a_gapless_cycle_advances_without_a_load",
    spinning(3),
    vec![near_the_end(), mark_fires(), handed_off(), near_the_end(), mark_fires()]
)]
#[case::repeat_one_preloads_and_hands_off_to_the_same_track(
    "repeat_one_preloads_and_hands_off_to_the_same_track",
    repeating(spinning_at(3, 1), RepeatMode::One),
    vec![near_the_end(), mark_fires(), handed_off()]
)]
#[case::repeat_one_preempts_a_queued_track(
    "repeat_one_preempts_a_queued_track",
    queued(repeating(spinning_at(3, 1), RepeatMode::One), &[2]),
    vec![near_the_end(), mark_fires()]
)]
#[case::repeat_one_reloads_the_same_track_when_it_ends(
    "repeat_one_reloads_the_same_track_when_it_ends",
    repeating(spinning_at(3, 1), RepeatMode::One),
    vec![ended()]
)]
#[case::repeat_one_preempts_the_queue_when_a_track_ends(
    "repeat_one_preempts_the_queue_when_a_track_ends",
    queued(repeating(spinning_at(3, 1), RepeatMode::One), &[0]),
    vec![ended()]
)]
#[case::a_manual_skip_ignores_repeat_one(
    "a_manual_skip_ignores_repeat_one",
    repeating(spinning_at(3, 1), RepeatMode::One),
    vec![skip()]
)]
#[case::a_queued_track_is_consumed_once_then_the_playlist_resumes(
    "a_queued_track_is_consumed_once_then_the_playlist_resumes",
    queued(spinning_at(4, 0), &[2]),
    vec![near_the_end(), mark_fires(), handed_off(), ended()]
)]
#[case::a_queued_track_plays_before_the_playlists_own_next(
    "a_queued_track_plays_before_the_playlists_own_next",
    spinning_at(3, 0),
    vec![enqueue(2), ended()]
)]
#[case::a_skip_takes_the_queue_head_and_moves_the_playlist_onto_it(
    "a_skip_takes_the_queue_head_and_moves_the_playlist_onto_it",
    spinning_at(3, 0),
    vec![enqueue(2), skip()]
)]
#[case::a_hand_off_at_the_end_of_the_playlist_keeps_the_track(
    "a_hand_off_at_the_end_of_the_playlist_keeps_the_track",
    spinning_at(2, 1),
    vec![handed_off()]
)]
#[case::a_hand_off_adopts_the_pin_not_a_queue_edit_made_since(
    "a_hand_off_adopts_the_pin_not_a_queue_edit_made_since",
    spinning(2),
    vec![near_the_end(), mark_fires(), enqueue(0), handed_off()]
)]
#[case::a_manual_skip_supersedes_the_pin_it_overtook(
    "a_manual_skip_supersedes_the_pin_it_overtook",
    spinning(4),
    vec![near_the_end(), mark_fires(), skip(), skip(), acknowledged(), handed_off()]
)]
#[case::stopping_drops_the_pin(
    "stopping_drops_the_pin",
    spinning(3),
    vec![near_the_end(), mark_fires(), Message::Playback(PlaybackRequest::Stop)]
)]
#[case::shuffle_without_an_order_advances_linearly(
    "shuffle_without_an_order_advances_linearly",
    {
        let mut model = spinning_at(4, 0);
        model.playlist.play_order = PlayOrder::ShufflePending;
        model
    },
    vec![ended()]
)]
#[case::an_installed_shuffle_order_is_what_advance_follows(
    "an_installed_shuffle_order_is_what_advance_follows",
    spinning_at(4, 0),
    vec![shuffle(), shuffled(vec![2, 0, 3, 1]), ended(), acknowledged(), ended()]
)]
#[case::a_shuffle_order_visits_every_track_once(
    "a_shuffle_order_visits_every_track_once",
    repeating(spinning_at(4, 0), RepeatMode::All),
    {
        let mut messages = vec![shuffle(), shuffled(vec![3, 1, 2, 0])];
        messages.extend(a_lap_of(3));
        messages
    }
)]
#[case::shuffle_wraps_at_the_end_of_its_order_with_repeat_off(
    "shuffle_wraps_at_the_end_of_its_order_with_repeat_off",
    {
        let mut model = spinning_at(4, 1);
        model.playlist.play_order = PlayOrder::Shuffle(vec![2, 0, 3, 1]);
        model
    },
    vec![skip()]
)]
#[case::toggling_shuffle_leaves_a_pin_the_engine_already_committed_to(
    "toggling_shuffle_leaves_a_pin_the_engine_already_committed_to",
    {
        let mut model = spinning_at(4, 0);
        model.playlist.play_order = PlayOrder::Shuffle(vec![0, 2, 1, 3]);
        model
    },
    vec![near_the_end(), mark_fires(), shuffle(), handed_off()]
)]
#[case::cycling_repeat_wraps_back_to_off(
    "cycling_repeat_wraps_back_to_off",
    Model::default(),
    vec![
        Message::Playback(PlaybackRequest::CycleRepeat),
        Message::Playback(PlaybackRequest::CycleRepeat),
        Message::Playback(PlaybackRequest::CycleRepeat),
    ]
)]
#[case::the_remotes_play_starts_a_stopped_player_and_leaves_a_playing_one(
    "the_remotes_play_starts_a_stopped_player_and_leaves_a_playing_one",
    model_with_tracks(3),
    vec![media(PlaybackRequest::Play), acknowledged(), media(PlaybackRequest::Play)]
)]
#[case::the_remotes_pause_pauses_once(
    "the_remotes_pause_pauses_once",
    spinning_at(3, 0),
    vec![media(PlaybackRequest::Pause), media(PlaybackRequest::Pause)]
)]
#[case::the_remotes_play_pause_flips(
    "the_remotes_play_pause_flips",
    model_with_tracks(3),
    vec![media(PlaybackRequest::Toggle), acknowledged(), media(PlaybackRequest::Toggle)]
)]
#[case::the_remotes_next_and_prev_walk_the_playlist(
    "the_remotes_next_and_prev_walk_the_playlist",
    spinning_at(3, 0),
    vec![media(PlaybackRequest::Next), media(PlaybackRequest::Prev)]
)]
#[case::one_ab_press_marks_the_start(
    "one_ab_press_marks_the_start",
    spinning_past(10),
    vec![mark_ab()]
)]
#[case::a_second_ab_press_past_the_start_closes_the_loop(
    "a_second_ab_press_past_the_start_closes_the_loop",
    spinning_past(10),
    vec![mark_ab(), Message::Audio(AudioEvent::Playhead(Duration::from_secs(20))), mark_ab()]
)]
#[case::a_second_ab_press_before_the_start_waits(
    "a_second_ab_press_before_the_start_waits",
    spinning_past(10),
    vec![mark_ab(), Message::Audio(AudioEvent::Playhead(Duration::from_secs(5))), mark_ab()]
)]
#[case::a_third_ab_press_clears_the_loop(
    "a_third_ab_press_clears_the_loop",
    spinning_past(10),
    vec![
        mark_ab(),
        Message::Audio(AudioEvent::Playhead(Duration::from_secs(20))),
        mark_ab(),
        mark_ab(),
    ]
)]
#[case::a_track_change_clears_the_loop_it_was_marked_on(
    "a_track_change_clears_the_loop_it_was_marked_on",
    {
        let mut model = spinning_at(3, 0);
        model.transport.ab = Some(AbLoop::Full {
            a: Duration::from_secs(5),
            b: Duration::from_secs(15),
        });
        model
    },
    vec![skip()]
)]
#[case::cycling_sleep_walks_the_presets_then_switches_off(
    "cycling_sleep_walks_the_presets_then_switches_off",
    Model::default(),
    vec![
        Message::Playback(PlaybackRequest::CycleSleep),
        Message::Playback(PlaybackRequest::CycleSleep),
        Message::Playback(PlaybackRequest::CycleSleep),
        Message::Playback(PlaybackRequest::CycleSleep),
    ]
)]
#[case::nudging_speed_up_saturates_at_the_top(
    "nudging_speed_up_saturates_at_the_top",
    Model::default(),
    vec![nudge_speed(1), nudge_speed(1), nudge_speed(1), nudge_speed(1), nudge_speed(1)]
)]
#[case::nudging_speed_down_steps(
    "nudging_speed_down_steps",
    Model::default(),
    vec![nudge_speed(-1)]
)]
#[case::a_track_change_leaves_the_speed_alone(
    "a_track_change_leaves_the_speed_alone",
    {
        let mut model = model_with_tracks(3);
        model.transport.speed = kernel::Speed::clamped(2.0);
        model
    },
    vec![Message::Playback(PlaybackRequest::Toggle), skip()]
)]
fn router_trace(
    #[case] name: &str,
    #[case] model: Model,
    #[case] messages: Vec<Message>,
) {
    insta::assert_debug_snapshot!(name, walked(model, messages));
}

#[rstest]
#[case::confirm_delete_on_an_empty_playlist_never_opens(
    Model::default(),
    open(OverlayName::ConfirmDelete),
    Rejection::Overlay(OverlayRejection::NoTrack)
)]
#[case::track_details_with_nothing_selected_and_nothing_playing_never_opens(
    Model::default(),
    open(OverlayName::TrackDetails),
    Rejection::Overlay(OverlayRejection::NoTrack)
)]
#[case::closing_nothing_is_refused(
    moon_library(),
    close(),
    Rejection::Overlay(OverlayRejection::WhileClosed)
)]
#[case::history_enqueue_against_an_empty_log_selects_nothing(
    logged(&[], &[]),
    history_enqueue(),
    Rejection::Overlay(OverlayRejection::History(HistoryRejection::NothingSelected))
)]
#[case::the_remotes_seek_while_stopped_is_refused(
    Model::default(),
    media(PlaybackRequest::SeekForward),
    Rejection::Player(PlayerRejection::Stopped)
)]
#[case::a_refused_key_keeps_the_toast_up(
    toasted(),
    media(PlaybackRequest::SeekForward),
    Rejection::Player(PlayerRejection::Stopped)
)]
fn a_refused_message_leaves_the_model_alone(
    #[case] mut model: Model,
    #[case] message: Message,
    #[case] rejection: Rejection,
) {
    let before = format!("{model:?}");
    assert_eq!(
        update(&mut model, message, Moment::default()).err(),
        Some(rejection)
    );
    assert_eq!(format!("{model:?}"), before);
}
