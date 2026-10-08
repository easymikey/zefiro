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

fn cell<T>(doorbell_sender: Sender<()>) -> (LatestSender<T>, LatestReceiver<T>) {
    let value = Arc::new(ArcSwapOption::empty());
    (
        LatestSender {
            value: Arc::clone(&value),
            doorbell_sender,
        },
        LatestReceiver { value },
    )
}

#[must_use]
pub fn latest_channels() -> (LatestSenders, LatestReceivers, Receiver<()>) {
    let (doorbell_sender, doorbell) = bounded(1);
    let (theme_sender, theme_receiver) = cell(doorbell_sender.clone());
    let (appearance_sender, appearance_receiver) = cell(doorbell_sender.clone());
    let (cover_sender, cover_receiver) = cell(doorbell_sender);
    let latest_senders = LatestSenders {
        theme_sender,
        appearance_sender,
        cover_sender,
    };
    let latest_receivers = LatestReceivers {
        theme_receiver,
        appearance_receiver,
        cover_receiver,
    };
    (latest_senders, latest_receivers, doorbell)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;
    use library::cover::{CoverDecoded, CoverLookup};

    use crate::latest::cell;

    #[test]
    fn a_reading_sees_only_the_latest_value() {
        let (doorbell_sender, _doorbell) = bounded(1);
        let (latest_sender, reading) = cell::<i32>(doorbell_sender);
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
        let (doorbell_sender, _doorbell) = bounded(1);
        let (latest_sender, reading) = cell::<CoverDecoded>(doorbell_sender);
        latest_sender.publish(stub_decoded("first.mp3"));
        latest_sender.publish(stub_decoded("second.mp3"));
        let installed = reading.take().unwrap();
        assert_eq!(installed.path, std::path::PathBuf::from("second.mp3"));
        assert!(reading.take().is_none());
    }

    #[test]
    fn a_full_doorbell_is_success() {
        let (doorbell_sender, doorbell) = bounded(1);
        let (latest_sender, _reading) = cell::<i32>(doorbell_sender);
        latest_sender.publish(1);
        latest_sender.publish(2);
        assert_eq!(doorbell.try_iter().count(), 1);
    }
}
