use kernel::{
    domain::{
        key::{Key, KeyCode},
        keymap::KeymapOverrides,
    },
    update::keymap::{bindings::Keymap, chord::KeyBinding},
};

pub(crate) fn bindings(keymap_overrides: &KeymapOverrides) -> Vec<KeyBinding> {
    Keymap::new(keymap_overrides.clone()).bindings().to_vec()
}

pub(crate) fn character(letter: char) -> Key {
    Key::plain(KeyCode::Char(letter))
}
