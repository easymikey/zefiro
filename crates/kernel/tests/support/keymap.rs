use kernel::{
    domain::{
        key::{Key, KeyCode},
        keymap::KeymapOverrides,
    },
    update::keymap::{bindings::Keymap, chord::KeyBinding},
};

pub(crate) fn bindings(config: &KeymapOverrides) -> Vec<KeyBinding> {
    Keymap::new(config.clone()).bindings().to_vec()
}

pub(crate) fn character(letter: char) -> Key {
    Key::plain(KeyCode::Char(letter))
}
