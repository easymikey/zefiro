use kernel::domain::{
    chord::Chord,
    key::{Key, Modifiers},
};
use proptest::prelude::{prop_assert_eq, proptest};

use crate::support::strategies::unmodified_key_code;

proptest! {
    #[test]
    fn an_unmodified_chord_round_trips_through_its_own_spelling(code in unmodified_key_code()) {
        let chord = Chord::Key(Key { code, modifiers: Modifiers::NONE });
        prop_assert_eq!(chord.to_string().parse::<Chord>(), Ok(chord));
    }
}
