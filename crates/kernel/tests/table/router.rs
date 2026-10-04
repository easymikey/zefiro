use std::time::Duration;

use kernel::{
    cmd::Effect,
    domain::{
        bounded::Bounded,
        cue::Cue,
        cursor::Cursor,
        direction::Direction,
        index::ViewIndex,
        model::Model,
        overlay::{DeleteCandidate, Overlay, OverlayName},
        player::{AbLoop, Player},
        playlist::{PlayOrder, RepeatMode},
        revision::Revision,
        time::Moment,
        transport::Transport,
    },
    message::{AudioEvent, Message, PlaybackRequest, Timer},
    update::{machine::Unhandled, update},
};
use rstest::{Context, rstest};

use crate::support::{
    model_playing_at,
    model_with_tracks,
    playing_model,
    router::{
        a_lap_of,
        acknowledged,
        close,
        confirm,
        cover_side_known,
        cover_side_unknown,
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
        step_speed,
        text_char,
        toasted,
        typed,
    },
};

type Step = (
    Message,
    Vec<Effect>,
    Player,
    Option<Overlay>,
    Cursor,
    Cursor,
    Vec<kernel::domain::track::TrackRef>,
    RepeatMode,
    PlayOrder,
    Transport,
    Vec<kernel::domain::toast::Toast>,
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
                cmd.into_iter().collect(),
                model.player.clone(),
                model.workspace.overlay.clone(),
                model.workspace.browse.cursor,
                model.playlist.cursor,
                model.queue.clone(),
                model.playlist.repeat,
                model.playlist.play_order.clone(),
                model.transport.clone(),
                model.workspace.toasts.clone(),
            )
        })
        .collect()
}

fn resolved(message: Message, model: &Model) -> Message {
    if let Message::Elapsed(Timer::Lookahead(placeholder)) = message
        && placeholder == Revision::default()
    {
        return Message::Elapsed(Timer::Lookahead(model.revisions.lookahead));
    }
    message
}

#[rstest]
#[case::search_typing_reranks_and_backspace_widens_it_back(
    moon_library(),
    search_moon(vec![search_backspace(), search_backspace(), search_backspace(), search_backspace()])
)]
#[case::search_confirm_plays_the_track_the_selected_match_resolves_to(
    moon_library(),
    search_moon(vec![search_nav(Direction::Next), confirm()])
)]
#[case::search_enqueue_queues_the_resolved_track_and_stays_open(
    moon_library(),
    search_moon(vec![search_nav(Direction::Next), search_enqueue()])
)]
#[case::search_esc_leaves_the_browse_cursor_where_it_was(
    moon_library_selecting(2),
    search_moon(vec![search_nav(Direction::Next), close()])
)]
#[case::help_opens_over_the_browse_cursor_and_closes_off_it(
    moon_library_selecting(2),
    vec![open(OverlayName::Help), close()]
)]
#[case::confirm_delete_captures_the_selected_track_and_trashes_it(
    moon_library_scanned(),
    vec![open(OverlayName::ConfirmDelete), confirm()]
)]
#[case::confirm_delete_cancelled_trashes_nothing(
    moon_library_scanned(),
    vec![open(OverlayName::ConfirmDelete), close()]
)]
#[case::track_details_shows_the_selected_playlist_track(
    moon_library_selecting(1),
    vec![open(OverlayName::TrackDetails), close()]
)]
#[case::track_details_falls_back_to_the_playing_track(
    playing_nothing_selected(Duration::from_secs(100)),
    vec![open(OverlayName::TrackDetails), close()]
)]
#[case::opening_history_loads_the_log(
    logged(&["/m/a.flac"], &["/m/a.flac"]),
    vec![open(OverlayName::History), close()]
)]
#[case::history_enqueue_queues_the_library_track_at_the_selected_path(
    logged(&["/m/a.flac", "/m/b.flac"], &["/m/a.flac", "/m/b.flac"]),
    vec![history_enqueue()]
)]
#[case::history_enqueue_of_a_path_no_longer_in_the_library_says_so(
    logged(&["/m/gone.flac"], &[]),
    vec![history_enqueue()]
)]
#[case::jump_confirm_clamps_the_parsed_target_to_the_track(
    playing_nothing_selected(Duration::from_secs(100)),
    {
        let mut messages = vec![open(OverlayName::JumpToTime)];
        messages.extend(typed("10:00", jump_char));
        messages.push(confirm());
        messages
    }
)]
#[case::saving_a_playlist_confirms_the_validated_name(
    model_with_tracks(1),
    {
        let mut messages = vec![open(OverlayName::SavePlaylist)];
        messages.extend(typed("mix", text_char));
        messages.push(confirm());
        messages
    }
)]
#[case::saving_a_playlist_under_a_rejected_name_stays_open_with_the_error(
    model_with_tracks(1),
    {
        let mut messages = vec![open(OverlayName::SavePlaylist)];
        messages.extend(typed("...", text_char));
        messages.push(confirm());
        messages
    }
)]
#[case::settings_closes_like_every_other_overlay(
    Model::default(),
    vec![open(OverlayName::Settings), close()]
)]
#[case::a_gapless_cycle_advances_without_a_load(
    playing_model(3),
    vec![near_the_end(), mark_fires(), handed_off(), near_the_end(), mark_fires()]
)]
#[case::a_known_cover_side_prefetches_the_next_cover_with_the_lookahead(
    playing_model(3),
    vec![cover_side_known(), near_the_end(), mark_fires()]
)]
#[case::an_unknown_cover_side_prefetches_no_cover_with_the_lookahead(
    playing_model(3),
    vec![cover_side_unknown(), near_the_end(), mark_fires()]
)]
#[case::repeat_one_preloads_and_hands_off_to_the_same_track(
    repeating(model_playing_at(3, 1, Duration::ZERO), RepeatMode::One),
    vec![near_the_end(), mark_fires(), handed_off()]
)]
#[case::repeat_one_preempts_a_queued_track(
    queued(repeating(model_playing_at(3, 1, Duration::ZERO), RepeatMode::One), &[2]),
    vec![near_the_end(), mark_fires()]
)]
#[case::repeat_one_reloads_the_same_track_when_it_ends(
    repeating(model_playing_at(3, 1, Duration::ZERO), RepeatMode::One),
    vec![ended()]
)]
#[case::repeat_one_preempts_the_queue_when_a_track_ends(
    queued(repeating(model_playing_at(3, 1, Duration::ZERO), RepeatMode::One), &[0]),
    vec![ended()]
)]
#[case::a_manual_skip_ignores_repeat_one(
    repeating(model_playing_at(3, 1, Duration::ZERO), RepeatMode::One),
    vec![skip()]
)]
#[case::a_queued_track_is_consumed_once_then_the_playlist_resumes(
    queued(model_playing_at(4, 0, Duration::ZERO), &[2]),
    vec![near_the_end(), mark_fires(), handed_off(), ended()]
)]
#[case::a_queued_track_plays_before_the_playlists_own_next(
    model_playing_at(3, 0, Duration::ZERO),
    vec![enqueue(2), ended()]
)]
#[case::a_skip_takes_the_queue_head_and_moves_the_playlist_onto_it(
    model_playing_at(3, 0, Duration::ZERO),
    vec![enqueue(2), skip()]
)]
#[case::a_hand_off_at_the_end_of_the_playlist_keeps_the_track(
    model_playing_at(2, 1, Duration::ZERO),
    vec![handed_off()]
)]
#[case::a_hand_off_adopts_the_pin_not_a_queue_edit_made_since(
    playing_model(2),
    vec![near_the_end(), mark_fires(), enqueue(0), handed_off()]
)]
#[case::a_manual_skip_supersedes_the_pin_it_overtook(
    playing_model(4),
    vec![near_the_end(), mark_fires(), skip(), skip(), acknowledged(), handed_off()]
)]
#[case::stopping_drops_the_pin(
    playing_model(3),
    vec![near_the_end(), mark_fires(), Message::Playback(PlaybackRequest::Stop)]
)]
#[case::shuffle_without_an_order_advances_linearly(
    {
        let mut model = model_playing_at(4, 0, Duration::ZERO);
        model.playlist.play_order = PlayOrder::ShufflePending;
        model
    },
    vec![ended()]
)]
#[case::an_installed_shuffle_order_is_what_advance_follows(
    model_playing_at(4, 0, Duration::ZERO),
    vec![shuffle(), shuffled(vec![2, 0, 3, 1]), ended(), acknowledged(), ended()]
)]
#[case::a_shuffle_order_visits_every_track_once(
    repeating(model_playing_at(4, 0, Duration::ZERO), RepeatMode::All),
    {
        let mut messages = vec![shuffle(), shuffled(vec![3, 1, 2, 0])];
        messages.extend(a_lap_of(3));
        messages
    }
)]
#[case::shuffle_wraps_at_the_end_of_its_order_with_repeat_off(
    {
        let mut model = model_playing_at(4, 1, Duration::ZERO);
        model.playlist.play_order = PlayOrder::Shuffle([2, 0, 3, 1].map(ViewIndex::new).to_vec());
        model
    },
    vec![skip()]
)]
#[case::toggling_shuffle_leaves_a_pin_the_engine_already_committed_to(
    {
        let mut model = model_playing_at(4, 0, Duration::ZERO);
        model.playlist.play_order = PlayOrder::Shuffle([0, 2, 1, 3].map(ViewIndex::new).to_vec());
        model
    },
    vec![near_the_end(), mark_fires(), shuffle(), handed_off()]
)]
#[case::cycling_repeat_wraps_back_to_off(
    Model::default(),
    vec![
        Message::Playback(PlaybackRequest::CycleRepeat),
        Message::Playback(PlaybackRequest::CycleRepeat),
        Message::Playback(PlaybackRequest::CycleRepeat),
    ]
)]
#[case::the_remotes_play_starts_a_stopped_player(
    model_with_tracks(3),
    vec![media(PlaybackRequest::Play), acknowledged()]
)]
#[case::the_remotes_pause_pauses(
    model_playing_at(3, 0, Duration::ZERO),
    vec![media(PlaybackRequest::Pause)]
)]
#[case::the_remotes_play_pause_flips(
    model_with_tracks(3),
    vec![media(PlaybackRequest::Toggle), acknowledged(), media(PlaybackRequest::Toggle)]
)]
#[case::the_remotes_next_and_prev_walk_the_playlist(
    model_playing_at(3, 0, Duration::ZERO),
    vec![media(PlaybackRequest::Next), media(PlaybackRequest::Previous)]
)]
#[case::one_ab_press_marks_the_start(
    model_playing_at(1, 0, Duration::from_secs(10)),
    vec![mark_ab()]
)]
#[case::a_second_ab_press_past_the_start_closes_the_loop(
    model_playing_at(1, 0, Duration::from_secs(10)),
    vec![mark_ab(), Message::Audio(AudioEvent::Playhead(Duration::from_secs(20))), mark_ab()]
)]
#[case::a_second_ab_press_before_the_start_waits(
    model_playing_at(1, 0, Duration::from_secs(10)),
    vec![mark_ab(), Message::Audio(AudioEvent::Playhead(Duration::from_secs(5))), mark_ab()]
)]
#[case::a_third_ab_press_clears_the_loop(
    model_playing_at(1, 0, Duration::from_secs(10)),
    vec![
        mark_ab(),
        Message::Audio(AudioEvent::Playhead(Duration::from_secs(20))),
        mark_ab(),
        mark_ab(),
    ]
)]
#[case::a_track_change_clears_the_loop_it_was_marked_on(
    {
        let mut model = model_playing_at(3, 0, Duration::ZERO);
        model.transport.ab_loop = Some(AbLoop::Full {
            a: Duration::from_secs(5),
            b: Duration::from_secs(15),
        });
        model
    },
    vec![skip()]
)]
#[case::cycling_sleep_walks_the_presets_then_switches_off(
    Model::default(),
    vec![
        Message::Playback(PlaybackRequest::CycleSleep),
        Message::Playback(PlaybackRequest::CycleSleep),
        Message::Playback(PlaybackRequest::CycleSleep),
        Message::Playback(PlaybackRequest::CycleSleep),
    ]
)]
#[case::stepping_speed_up_saturates_at_the_top(
    Model::default(),
    vec![step_speed(Direction::Next), step_speed(Direction::Next), step_speed(Direction::Next), step_speed(Direction::Next), step_speed(Direction::Next)]
)]
#[case::stepping_speed_down_steps(
    Model::default(),
    vec![step_speed(Direction::Previous)]
)]
#[case::a_track_change_leaves_the_speed_alone(
    {
        let mut model = model_with_tracks(3);
        model.transport.speed = kernel::domain::speed::Speed::clamped(2.0);
        model
    },
    vec![Message::Playback(PlaybackRequest::Toggle), skip()]
)]
fn router_trace(
    #[context] case: Context,
    #[case] model: Model,
    #[case] messages: Vec<Message>,
) {
    let name = case.description.expect("every case is named");
    insta::assert_debug_snapshot!(name, walked(model, messages));
}

#[rstest]
#[case::confirm_delete_on_an_empty_playlist_never_opens(
    Model::default(),
    open(OverlayName::ConfirmDelete),
    Unhandled
)]
#[case::track_details_with_nothing_selected_and_nothing_playing_never_opens(
    Model::default(),
    open(OverlayName::TrackDetails),
    Unhandled
)]
#[case::closing_nothing_is_refused(moon_library(), close(), Unhandled)]
#[case::history_enqueue_against_an_empty_log_selects_nothing(
    logged(&[], &[]),
    history_enqueue(),
    Unhandled
)]
#[case::the_remotes_seek_while_stopped_is_refused(
    Model::default(),
    media(PlaybackRequest::SeekForward),
    Unhandled
)]
#[case::the_remotes_play_while_playing_is_refused(
    model_playing_at(3, 0, Duration::ZERO),
    media(PlaybackRequest::Play),
    Unhandled
)]
#[case::the_remotes_pause_while_stopped_is_refused(
    model_with_tracks(3),
    media(PlaybackRequest::Pause),
    Unhandled
)]
#[case::a_refused_key_keeps_the_toast_up(
    toasted(),
    media(PlaybackRequest::SeekForward),
    Unhandled
)]
fn a_refused_message_leaves_the_model_alone(
    #[case] mut model: Model,
    #[case] message: Message,
    #[case] rejection: Unhandled,
) {
    let before = format!("{model:?}");
    assert_eq!(
        update(&mut model, message, Moment::default()).err(),
        Some(rejection)
    );
    assert_eq!(format!("{model:?}"), before);
}

#[test]
fn a_refused_follow_up_keeps_the_parents_effects() {
    let mut model = Model::default();
    model.workspace.overlay = Some(Overlay::ConfirmDelete(DeleteCandidate {
        source: kernel::domain::track::TrackRef::Local("/music/gone.flac".into()),
        title: "Gone".to_string(),
        artist: String::new(),
    }));

    let effects = update(&mut model, confirm(), Moment::default());

    assert_eq!(effects, Ok(vec![Effect::Animate(Cue::OverlayClosed)]));
    assert_eq!(model.workspace.overlay, None);
}
