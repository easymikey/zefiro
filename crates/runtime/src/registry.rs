use kernel::domain::driver::DriverName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hosting {
    Worker,
    WorkerWithMainLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Every,
    Macos,
}

impl Platform {
    #[must_use]
    pub(crate) const fn present(self) -> bool {
        match self {
            Platform::Every => true,
            Platform::Macos => cfg!(target_os = "macos"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DriverRow {
    pub(crate) driver: DriverName,
    pub(crate) thread_name: &'static str,
    pub(crate) hosting: Hosting,
    pub(crate) platform: Platform,
}

pub(crate) const REGISTRY: [DriverRow; 4] = [
    DriverRow {
        driver: DriverName::Audio,
        thread_name: "sifr-audio",
        hosting: Hosting::Worker,
        platform: Platform::Every,
    },
    DriverRow {
        driver: DriverName::Macos,
        thread_name: "sifr-macos",
        hosting: Hosting::WorkerWithMainLoop,
        platform: Platform::Macos,
    },
    DriverRow {
        driver: DriverName::Library,
        thread_name: "sifr-library",
        hosting: Hosting::Worker,
        platform: Platform::Every,
    },
    DriverRow {
        driver: DriverName::Config,
        thread_name: "sifr-config",
        hosting: Hosting::Worker,
        platform: Platform::Every,
    },
];

pub(crate) const fn row(driver: DriverName) -> &'static DriverRow {
    let [audio, macos, library, config] = &REGISTRY;
    match driver {
        DriverName::Audio => audio,
        DriverName::Macos => macos,
        DriverName::Library => library,
        DriverName::Config => config,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::driver::DriverName;

    use crate::registry::{REGISTRY, row};

    #[test]
    fn every_driver_has_one_row() {
        assert_eq!(
            REGISTRY.map(|entry| entry.driver),
            [
                DriverName::Audio,
                DriverName::Macos,
                DriverName::Library,
                DriverName::Config
            ]
        );
        for driver in DriverName::ALL {
            assert_eq!(row(driver).driver, driver);
        }
    }
}
