use kernel::domain::cursor::Cursor;
use proptest::prelude::{Just, Strategy, any, prop_assert, prop_assert_eq, proptest};
use rstest::rstest;

fn invariant_holds(cursor: Cursor) -> bool {
    if cursor.is_empty() {
        cursor.index() == 0
    } else {
        cursor.index() < cursor.len()
    }
}

proptest! {
    #[test]
    fn step_forward_then_backward_is_identity_away_from_edges(
        (len, index) in (3usize..12usize)
            .prop_flat_map(|len| (Just(len), 1usize..(len - 1))),
    ) {
        let cursor = Cursor::with_len(len).at(index);
        let round_trip = cursor.step(1).step(-1);
        prop_assert_eq!(round_trip, cursor);
    }

    #[test]
    fn resize_to_zero_is_empty(len in 0usize..12, index in 0usize..12) {
        let cursor = Cursor::with_len(len).at(index).resize(0);
        prop_assert!(cursor.is_empty());
    }

    #[test]
    fn invariant_survives_any_step_and_resize_sequence(
        start_len in 0usize..12,
        start_index in 0usize..12,
        ops in proptest::collection::vec(
            (any::<i8>(), 0usize..12),
            0..20,
        ),
    ) {
        let mut cursor = Cursor::with_len(start_len).at(start_index);
        prop_assert!(invariant_holds(cursor));
        for (delta, resize_len) in ops {
            cursor = cursor.step(isize::from(delta));
            prop_assert!(invariant_holds(cursor));

            cursor = cursor.resize(resize_len);
            prop_assert!(invariant_holds(cursor));
        }
    }
}

struct StepRow {
    index: usize,
    len: usize,
    delta: isize,
    expected_index: usize,
}

#[rstest]
#[case(StepRow { index: 0, len: 5, delta: -1, expected_index: 0 })]
#[case(StepRow { index: 4, len: 5, delta: 1, expected_index: 4 })]
fn step_clamps_at_both_ends(#[case] row: StepRow) {
    let cursor = Cursor::with_len(row.len).at(row.index).step(row.delta);
    assert_eq!(cursor.index(), row.expected_index);
}

#[test]
fn get_returns_none_for_an_empty_cursor() {
    let items: [u8; 0] = [];
    assert_eq!(Cursor::new(0).get(&items), None);
}

#[test]
fn get_returns_the_item_at_index() {
    let items = ["a", "b", "c"];
    let cursor = Cursor::with_len(items.len()).at(1);
    assert_eq!(cursor.get(&items), Some(&"b"));
}
