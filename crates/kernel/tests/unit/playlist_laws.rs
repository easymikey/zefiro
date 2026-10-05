use std::time::Duration;

use kernel::domain::{
    cursor::Cursor,
    direction::Direction,
    index::ViewIndex,
    player::AbLoop,
    playlist::{PlayOrder, Playlist},
};
use proptest::prelude::{Just, prop_assert, prop_assert_eq, prop_oneof, proptest};

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
            PlayOrder::Shuffle((0..len).map(ViewIndex::new).collect())
        } else {
            PlayOrder::Linear
        };
        let mut playlist = Playlist {
            tracks,
            cursor: Cursor::at(len, start),
            play_order,
            repeat,
        };
        playlist.skip(direction);
        if len == 0 {
            prop_assert!(playlist.cursor.is_empty());
        } else {
            prop_assert!(playlist.cursor.index() < len);
        }
        if let PlayOrder::Shuffle(order) = &playlist.play_order {
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
            if let Some(AbLoop::Full { a, b }) = state {
                prop_assert!(b > a);
            }
        }
    }
}
