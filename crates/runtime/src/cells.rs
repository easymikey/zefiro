use std::sync::Arc;

use arc_swap::ArcSwapOption;
use config::{AppearanceFile, ThemeFile};
use crossbeam_channel::{Receiver, Sender, bounded};

use crate::library::cover::CoverDecoded;

#[derive(Debug)]
pub struct Latest<T> {
    slot: Arc<ArcSwapOption<T>>,
    doorbell: Sender<()>,
}

impl<T> Latest<T> {
    pub(crate) fn publish(&self, value: T) {
        self.slot.store(Some(Arc::new(value)));
        let _ = self.doorbell.try_send(());
    }
}

impl<T> Clone for Latest<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
            doorbell: self.doorbell.clone(),
        }
    }
}

#[derive(Debug)]
pub struct Reading<T> {
    slot: Arc<ArcSwapOption<T>>,
}

impl<T> Reading<T> {
    #[must_use]
    pub fn take(&self) -> Option<Arc<T>> {
        self.slot.swap(None)
    }
}

#[derive(Debug)]
pub struct Cells {
    pub theme: Reading<ThemeFile>,
    pub appearance: Reading<AppearanceFile>,
    pub cover: Reading<CoverDecoded>,
}

#[derive(Debug, Clone)]
pub struct Writers {
    pub theme: Latest<ThemeFile>,
    pub appearance: Latest<AppearanceFile>,
    pub cover: Latest<CoverDecoded>,
}

#[must_use]
pub fn cells() -> (Writers, Cells, Receiver<()>) {
    let (ring, doorbell) = bounded(1);
    let theme = Arc::new(ArcSwapOption::empty());
    let appearance = Arc::new(ArcSwapOption::empty());
    let cover = Arc::new(ArcSwapOption::empty());
    let writers = Writers {
        theme: Latest {
            slot: Arc::clone(&theme),
            doorbell: ring.clone(),
        },
        appearance: Latest {
            slot: Arc::clone(&appearance),
            doorbell: ring.clone(),
        },
        cover: Latest {
            slot: Arc::clone(&cover),
            doorbell: ring,
        },
    };
    let cells = Cells {
        theme: Reading { slot: theme },
        appearance: Reading { slot: appearance },
        cover: Reading { slot: cover },
    };
    (writers, cells, doorbell)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;

    use crate::{
        cells::{Latest, Reading},
        library::cover::{CoverDecoded, CoverOutcome},
    };

    fn pair<T>() -> (Latest<T>, Reading<T>, crossbeam_channel::Receiver<()>) {
        let (ring, doorbell) = bounded(1);
        let slot = std::sync::Arc::new(arc_swap::ArcSwapOption::empty());
        (
            Latest {
                slot: std::sync::Arc::clone(&slot),
                doorbell: ring,
            },
            Reading { slot },
            doorbell,
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
    fn a_full_doorbell_is_success() {
        let (latest, _reading, doorbell) = pair::<i32>();
        latest.publish(1);
        latest.publish(2);
        assert_eq!(doorbell.try_iter().count(), 1);
    }
}
