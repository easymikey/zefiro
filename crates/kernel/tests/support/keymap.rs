use kernel::{
    domain::{
        key::{Key, KeyCode},
        keymap::KeymapOverrides,
    },
    update::keymap::{bindings::Bindings, chord::KeyBinding},
};

pub(crate) fn bindings(config: &KeymapOverrides) -> Vec<KeyBinding> {
    Bindings::new(config).as_slice().to_vec()
}

pub(crate) fn character(letter: char) -> Key {
    Key::plain(KeyCode::Char(letter))
}
