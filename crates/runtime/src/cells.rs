use std::sync::Arc;

use arc_swap::ArcSwapOption;
use config::{AppearanceFile, ThemeFile};
use crossbeam_channel::{Receiver, Sender, bounded};

use crate::library::cover::CoverDecoded;

#[derive(Debug)]
pub struct LatestSender<T> {
    slot: Arc<ArcSwapOption<T>>,
    notify: Sender<()>,
}

impl<T> LatestSender<T> {
    pub(crate) fn publish(&self, value: T) {
        self.slot.store(Some(Arc::new(value)));
        let _ = self.notify.try_send(());
    }
}

impl<T> Clone for LatestSender<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
            notify: self.notify.clone(),
        }
    }
}

#[derive(Debug)]
pub struct LatestReceiver<T> {
    slot: Arc<ArcSwapOption<T>>,
}

impl<T> LatestReceiver<T> {
    #[must_use]
    pub fn take(&self) -> Option<Arc<T>> {
        self.slot.swap(None)
    }
}

#[derive(Debug)]
pub struct Receivers {
    pub theme: LatestReceiver<ThemeFile>,
    pub appearance: LatestReceiver<AppearanceFile>,
    pub cover: LatestReceiver<CoverDecoded>,
}

#[derive(Debug, Clone)]
pub struct Senders {
    pub theme: LatestSender<ThemeFile>,
    pub appearance: LatestSender<AppearanceFile>,
    pub cover: LatestSender<CoverDecoded>,
}

#[must_use]
pub fn cells() -> (Senders, Receivers, Receiver<()>) {
    let (ring, notified) = bounded(1);
    let theme = Arc::new(ArcSwapOption::empty());
    let appearance = Arc::new(ArcSwapOption::empty());
    let cover = Arc::new(ArcSwapOption::empty());
    let writers = Senders {
        theme: LatestSender {
            slot: Arc::clone(&theme),
            notify: ring.clone(),
        },
        appearance: LatestSender {
            slot: Arc::clone(&appearance),
            notify: ring.clone(),
        },
        cover: LatestSender {
            slot: Arc::clone(&cover),
            notify: ring,
        },
    };
    let cells = Receivers {
        theme: LatestReceiver { slot: theme },
        appearance: LatestReceiver { slot: appearance },
        cover: LatestReceiver { slot: cover },
    };
    (writers, cells, notified)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;

    use crate::{
        cells::{LatestReceiver, LatestSender},
        library::cover::{CoverDecoded, CoverOutcome},
    };

    fn pair<T>() -> (
        LatestSender<T>,
        LatestReceiver<T>,
        crossbeam_channel::Receiver<()>,
    ) {
        let (ring, notified) = bounded(1);
        let slot = std::sync::Arc::new(arc_swap::ArcSwapOption::empty());
        (
            LatestSender {
                slot: std::sync::Arc::clone(&slot),
                notify: ring,
            },
            LatestReceiver { slot },
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
            side: 64,
            outcome: CoverOutcome::NoArt,
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
