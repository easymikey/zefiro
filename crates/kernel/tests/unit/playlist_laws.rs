use std::time::Duration;

use kernel::domain::{
    cursor::Cursor,
    direction::Direction,
    index::ViewIndex,
    player::AbLoop,
    playlist::{PlayOrder, Playlist, RepeatMode},
};
use proptest::prelude::{Just, prop_assert, prop_assert_eq, prop_oneof, proptest};
use rstest::rstest;

use crate::support::{bare_track, strategies::repeat_mode};

proptest! {
    #[test]
    fn skip_keeps_the_cursor_in_range(
        len in 0usize..8,
        start in 0usize..8,
        repeat in repeat_mode(),
        shuffled in proptest::bool::ANY,
        direction in prop_oneof![Just(Direction::Next), Just(Direction::Previous)],
    ) {
        let tracks = (0..len).map(bare_track).collect::<Vec<_>>();
        let play_order = if shuffled && len > 0 {
            PlayOrder::Shuffled((0..len).map(ViewIndex::new).collect())
        } else {
            PlayOrder::Linear
        };
        let mut playlist = Playlist {
            tracks,
            cursor: Cursor::at(len, start),
            play_order,
            repeat_mode: repeat,
        };
        playlist.skip(direction);
        if len == 0 {
            prop_assert!(playlist.cursor.is_empty());
        } else {
            prop_assert!(playlist.cursor.index() < len);
        }
        if let PlayOrder::Shuffled(order) = &playlist.play_order {
            let mut sorted = order.clone();
            sorted.sort_unstable();
            prop_assert_eq!(sorted, (0..len).map(ViewIndex::new).collect::<Vec<_>>());
        }
    }

    #[test]
    fn ab_loop_mark_never_yields_a_full_loop_with_b_before_a(
        positions in proptest::collection::vec(0u64..60_000, 0..20),
    ) {
        let mut state: Option<AbLoop> = None;
        for millis in positions {
            state = AbLoop::mark(state, Duration::from_millis(millis));
            if let Some(AbLoop::BothMarked { loop_start, loop_end }) = state {
                prop_assert!(loop_end > loop_start);
            }
        }
    }
}

#[rstest]
#[case::repeat_all_wraps_past_the_last(RepeatMode::All, Direction::Next, Some(0))]
#[case::repeat_all_wraps_before_the_first(
    RepeatMode::All,
    Direction::Previous,
    Some(2)
)]
#[case::repeat_off_stops_at_the_last(RepeatMode::Off, Direction::Next, None)]
#[case::repeat_off_stops_at_the_first(RepeatMode::Off, Direction::Previous, None)]
#[case::repeat_one_stops_at_the_last(RepeatMode::One, Direction::Next, None)]
#[case::repeat_one_stops_at_the_first(RepeatMode::One, Direction::Previous, None)]
fn skip_at_the_edges_follows_the_repeat_mode(
    #[case] repeat_mode: RepeatMode,
    #[case] direction: Direction,
    #[case] expected: Option<usize>,
) {
    let start = match direction {
        Direction::Next => 2,
        Direction::Previous => 0,
    };
    let mut playlist = Playlist {
        tracks: (0..3).map(bare_track).collect(),
        cursor: Cursor::at(3, start),
        play_order: PlayOrder::Linear,
        repeat_mode,
    };
    let moved = playlist.skip(direction).is_some();
    assert_eq!(moved.then(|| playlist.cursor.index()), expected);
}
