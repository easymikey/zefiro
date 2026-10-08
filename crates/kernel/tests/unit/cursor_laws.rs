use kernel::domain::cursor::Cursor;
use proptest::prelude::{Just, Strategy, prop_assert, prop_assert_eq, proptest};

proptest! {
    #[test]
    fn step_forward_then_backward_is_identity_away_from_edges(
        (len, start_index) in (3usize..12usize)
            .prop_flat_map(|len| (Just(len), 1usize..(len - 1))),
    ) {
        let cursor = Cursor::at(len, start_index);
        let round_trip = cursor.step(1).step(-1);
        prop_assert_eq!(round_trip, cursor);
    }

    #[test]
    fn resize_to_zero_is_empty(len in 0usize..12, start_index in 0usize..12) {
        let cursor = Cursor::at(len, start_index).resize(0);
        prop_assert!(cursor.is_empty());
    }
}
