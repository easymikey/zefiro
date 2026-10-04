pub(crate) mod disk;
pub(crate) mod driver;
pub(crate) mod machine;
pub(crate) mod reload;
pub(crate) mod save_queue;
pub(crate) mod seen;
pub(crate) mod session;
pub(crate) mod watch;
pub(crate) mod write;

pub use ::config::{ConfigPaths, SeenTexts};

#[cfg(test)]
pub(crate) mod fixtures {
    use std::time::Duration;

    use crossbeam_channel::Receiver;

    use crate::config::{ConfigPaths, SeenTexts};

    pub(crate) const RECV_TIMEOUT: Duration = Duration::from_secs(2);
    pub(crate) const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    pub(crate) fn paths(directory: &tempfile::TempDir) -> ConfigPaths {
        ConfigPaths {
            config: directory.path().join("config.toml"),
            appearance: directory.path().join("sifr-ui.toml"),
            themes: directory.path().join("themes"),
            theme: Some("noir".to_string()),
            seen: SeenTexts::default(),
        }
    }

    pub(crate) fn drain<T>(receiver: &Receiver<T>) -> Vec<T> {
        let mut collected = vec![receiver.recv_timeout(RECV_TIMEOUT).unwrap()];
        while let Ok(item) = receiver.recv_timeout(SETTLE_TIMEOUT) {
            collected.push(item);
        }
        collected
    }
}
