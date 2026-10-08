use apple_native_keyring_store::keychain::Store;

pub fn set_default_store() {
    if let Ok(store) = Store::new() {
        keyring_core::set_default_store(store);
    }
}
