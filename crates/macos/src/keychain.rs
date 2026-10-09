use std::sync::OnceLock;

use apple_native_keyring_store::keychain::Store;
use security_framework::os::macos::keychain::{
    KeychainUserInteractionLock,
    SecKeychain,
};

static USER_INTERACTION_LOCK: OnceLock<KeychainUserInteractionLock> = OnceLock::new();

pub fn set_default_store() {
    if USER_INTERACTION_LOCK.get().is_none()
        && let Ok(lock) = SecKeychain::disable_user_interaction()
    {
        USER_INTERACTION_LOCK.get_or_init(|| lock);
    }
    if let Ok(store) = Store::new() {
        keyring_core::set_default_store(store);
    }
}
