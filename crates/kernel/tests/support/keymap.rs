use kernel::{
    Key,
    KeyCode,
    domain::KeymapOverrides,
    update::keymap::{Bindings, KeyBinding},
};

pub(crate) fn bindings(config: &KeymapOverrides) -> Vec<KeyBinding> {
    Bindings::new(config).as_slice().to_vec()
}

pub(crate) fn character(letter: char) -> Key {
    Key::plain(KeyCode::Char(letter))
}
