use kernel::domain::{
    chord::{Chord, ChordPrefix},
    key::{Key, KeyCode, Modifiers},
};
use proptest::prelude::{prop_assert_eq, proptest};

use crate::support::strategies::unmodified_key_code;

#[test]
fn gg_spells_the_g_sequence() {
    assert_eq!(
        "gg".parse::<Chord>(),
        Ok(Chord::Sequence {
            chord_prefix: ChordPrefix::G,
            key: Key::plain(KeyCode::Char('g')),
        })
    );
}

proptest! {
    #[test]
    fn an_unmodified_chord_round_trips_through_its_own_spelling(code in unmodified_key_code()) {
        let chord = Chord::Key(Key { code, modifiers: Modifiers::NONE });
        prop_assert_eq!(chord.to_string().parse::<Chord>(), Ok(chord));
    }
}
