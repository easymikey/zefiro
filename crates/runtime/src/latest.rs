use std::sync::Arc;

use arc_swap::ArcSwapOption;
use config::TomlTheme;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use library::CoverDecoded;

#[derive(Debug)]
pub struct LatestSender<T> {
    value: Arc<ArcSwapOption<T>>,
    notify: Sender<()>,
}

impl<T> LatestSender<T> {
    pub(crate) fn publish(&self, latest: T) {
        self.value.store(Some(Arc::new(latest)));
        match self.notify.try_send(()) {
            Ok(()) | Err(TrySendError::Full(()) | TrySendError::Disconnected(())) => {}
        }
    }
}

impl<T> Clone for LatestSender<T> {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
            notify: self.notify.clone(),
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
    pub theme: LatestReceiver<TomlTheme>,
    pub cover: LatestReceiver<CoverDecoded>,
}

#[derive(Debug, Clone)]
pub struct LatestSenders {
    pub theme: LatestSender<TomlTheme>,
    pub cover: LatestSender<CoverDecoded>,
}

#[must_use]
pub fn latest_channels() -> (LatestSenders, LatestReceivers, Receiver<()>) {
    let (notifier, notified) = bounded(1);
    let theme = Arc::new(ArcSwapOption::empty());
    let cover = Arc::new(ArcSwapOption::empty());
    let writers = LatestSenders {
        theme: LatestSender {
            value: Arc::clone(&theme),
            notify: notifier.clone(),
        },
        cover: LatestSender {
            value: Arc::clone(&cover),
            notify: notifier,
        },
    };
    let cells = LatestReceivers {
        theme: LatestReceiver { value: theme },
        cover: LatestReceiver { value: cover },
    };
    (writers, cells, notified)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;
    use library::{CoverArt, CoverDecoded};

    use crate::latest::{LatestReceiver, LatestSender};

    fn pair<T>() -> (
        LatestSender<T>,
        LatestReceiver<T>,
        crossbeam_channel::Receiver<()>,
    ) {
        let (notifier, notified) = bounded(1);
        let value = std::sync::Arc::new(arc_swap::ArcSwapOption::empty());
        (
            LatestSender {
                value: std::sync::Arc::clone(&value),
                notify: notifier,
            },
            LatestReceiver { value },
            notified,
        )
    }

    #[test]
    fn a_reading_sees_only_the_latest_value() {
        let (latest, reading, _doorbell) = pair::<i32>();
        latest.publish(1);
        latest.publish(2);
        latest.publish(3);
        assert_eq!(reading.take().map(|value| *value), Some(3));
        assert!(reading.take().is_none());
    }

    fn stub_decoded(path: &str) -> CoverDecoded {
        CoverDecoded {
            path: std::path::PathBuf::from(path),
            side: kernel::domain::geometry::Pixels(64),
            art: CoverArt::Missing,
        }
    }

    #[test]
    fn a_cover_cell_keeps_the_latest_decode() {
        let (latest, reading, _doorbell) = pair::<CoverDecoded>();
        latest.publish(stub_decoded("first.mp3"));
        latest.publish(stub_decoded("second.mp3"));
        let installed = reading.take().unwrap();
        assert_eq!(installed.path, std::path::PathBuf::from("second.mp3"));
        assert!(reading.take().is_none());
    }

    #[test]
    fn a_full_notify_is_success() {
        let (latest, _reading, notified) = pair::<i32>();
        latest.publish(1);
        latest.publish(2);
        assert_eq!(notified.try_iter().count(), 1);
    }
}
