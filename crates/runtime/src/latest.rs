use std::sync::Arc;

use arc_swap::ArcSwapOption;
use config::theme_file::TomlTheme;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use kernel::domain::appearance::Appearance;
use library::cover::CoverDecoded;

#[derive(Debug)]
pub(crate) struct LatestSender<T> {
    value: Arc<ArcSwapOption<T>>,
    doorbell_sender: Sender<()>,
}

impl<T> LatestSender<T> {
    pub(crate) fn publish(&self, latest: T) {
        self.value.store(Some(Arc::new(latest)));
        match self.doorbell_sender.try_send(()) {
            Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => {}
        }
    }
}

impl<T> Clone for LatestSender<T> {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
            doorbell_sender: self.doorbell_sender.clone(),
        }
    }
}

#[derive(Debug)]
pub struct LatestReceiver<T> {
    value: Arc<ArcSwapOption<T>>,
}

impl<T> LatestReceiver<T> {
    #[must_use]
    pub fn take(&self) -> Option<Arc<T>> {
        self.value.swap(None)
    }
}

#[derive(Debug)]
pub struct LatestReceivers {
    pub theme_receiver: LatestReceiver<TomlTheme>,
    pub appearance_receiver: LatestReceiver<Appearance>,
    pub cover_receiver: LatestReceiver<CoverDecoded>,
}

#[derive(Debug, Clone)]
pub struct LatestSenders {
    pub(crate) theme_sender: LatestSender<TomlTheme>,
    pub(crate) appearance_sender: LatestSender<Appearance>,
    pub(crate) cover_sender: LatestSender<CoverDecoded>,
}

#[must_use]
pub fn latest_channels() -> (LatestSenders, LatestReceivers, Receiver<()>) {
    let (doorbell_sender, doorbell) = bounded(1);
    let theme = Arc::new(ArcSwapOption::empty());
    let appearance = Arc::new(ArcSwapOption::empty());
    let cover = Arc::new(ArcSwapOption::empty());
    let latest_senders = LatestSenders {
        theme_sender: LatestSender {
            value: Arc::clone(&theme),
            doorbell_sender: doorbell_sender.clone(),
        },
        appearance_sender: LatestSender {
            value: Arc::clone(&appearance),
            doorbell_sender: doorbell_sender.clone(),
        },
        cover_sender: LatestSender {
            value: Arc::clone(&cover),
            doorbell_sender,
        },
    };
    let latest_receivers = LatestReceivers {
        theme_receiver: LatestReceiver { value: theme },
        appearance_receiver: LatestReceiver { value: appearance },
        cover_receiver: LatestReceiver { value: cover },
    };
    (latest_senders, latest_receivers, doorbell)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;
    use library::cover::{CoverDecoded, CoverLookup};

    use crate::latest::{LatestReceiver, LatestSender};

    fn pair<T>() -> (
        LatestSender<T>,
        LatestReceiver<T>,
        crossbeam_channel::Receiver<()>,
    ) {
        let (doorbell_sender, doorbell) = bounded(1);
        let value = std::sync::Arc::new(arc_swap::ArcSwapOption::empty());
        (
            LatestSender {
                value: std::sync::Arc::clone(&value),
                doorbell_sender,
            },
            LatestReceiver { value },
            doorbell,
        )
    }

    #[test]
    fn a_reading_sees_only_the_latest_value() {
        let (latest_sender, reading, _doorbell) = pair::<i32>();
        latest_sender.publish(1);
        latest_sender.publish(2);
        latest_sender.publish(3);
        assert_eq!(reading.take().map(|value| *value), Some(3));
        assert!(reading.take().is_none());
    }

    fn stub_decoded(path: &str) -> CoverDecoded {
        CoverDecoded {
            path: std::path::PathBuf::from(path),
            side: kernel::domain::geometry::Pixels(64),
            cover_lookup: CoverLookup::Missing,
        }
    }

    #[test]
    fn a_cover_cell_keeps_the_latest_decode() {
        let (latest_sender, reading, _doorbell) = pair::<CoverDecoded>();
        latest_sender.publish(stub_decoded("first.mp3"));
        latest_sender.publish(stub_decoded("second.mp3"));
        let installed = reading.take().unwrap();
        assert_eq!(installed.path, std::path::PathBuf::from("second.mp3"));
        assert!(reading.take().is_none());
    }

    #[test]
    fn a_full_doorbell_is_success() {
        let (latest_sender, _reading, doorbell) = pair::<i32>();
        latest_sender.publish(1);
        latest_sender.publish(2);
        assert_eq!(doorbell.try_iter().count(), 1);
    }
}
