use kernel::domain::driver::DriverName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Every,
    Macos,
}

impl Platform {
    #[must_use]
    pub(crate) const fn is_present(self) -> bool {
        match self {
            Platform::Every => true,
            Platform::Macos => cfg!(target_os = "macos"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DriverRow {
    pub(crate) driver_name: DriverName,
    pub(crate) thread_name: &'static str,
    pub(crate) platform: Platform,
}

pub(crate) const REGISTRY: [DriverRow; 5] = [
    DriverRow {
        driver_name: DriverName::Audio,
        thread_name: "zefiro-audio",
        platform: Platform::Every,
    },
    DriverRow {
        driver_name: DriverName::Macos,
        thread_name: "zefiro-macos",
        platform: Platform::Macos,
    },
    DriverRow {
        driver_name: DriverName::Library,
        thread_name: "zefiro-library",
        platform: Platform::Every,
    },
    DriverRow {
        driver_name: DriverName::Config,
        thread_name: "zefiro-config",
        platform: Platform::Every,
    },
    DriverRow {
        driver_name: DriverName::Remote,
        thread_name: "zefiro-remote",
        platform: Platform::Every,
    },
];

pub(crate) const fn row(driver_name: DriverName) -> &'static DriverRow {
    let [audio, macos, library, config, remote] = &REGISTRY;
    match driver_name {
        DriverName::Audio => audio,
        DriverName::Macos => macos,
        DriverName::Library => library,
        DriverName::Config => config,
        DriverName::Remote => remote,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::driver::DriverName;

    use crate::registry::{REGISTRY, row};

    #[test]
    fn every_driver_has_one_row() {
        assert_eq!(
            REGISTRY.map(|entry| entry.driver_name),
            [
                DriverName::Audio,
                DriverName::Macos,
                DriverName::Library,
                DriverName::Config,
                DriverName::Remote
            ]
        );
        for driver in DriverName::ALL {
            assert_eq!(row(driver).driver_name, driver);
        }
    }
}
