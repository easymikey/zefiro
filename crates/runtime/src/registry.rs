use kernel::domain::Driver;

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
    pub(crate) driver: Driver,
    pub(crate) thread_name: &'static str,
    pub(crate) hosting: Hosting,
    pub(crate) platform: Platform,
}

pub(crate) const REGISTRY: [DriverRow; 4] = [
    DriverRow {
        driver: Driver::Audio,
        thread_name: "sifr-audio",
        hosting: Hosting::Worker,
        platform: Platform::Every,
    },
    DriverRow {
        driver: Driver::Macos,
        thread_name: "sifr-macos",
        hosting: Hosting::WorkerWithMainLoop,
        platform: Platform::Macos,
    },
    DriverRow {
        driver: Driver::Library,
        thread_name: "sifr-library",
        hosting: Hosting::Worker,
        platform: Platform::Every,
    },
    DriverRow {
        driver: Driver::Config,
        thread_name: "sifr-config",
        hosting: Hosting::Worker,
        platform: Platform::Every,
    },
];

pub(crate) const fn row(driver: Driver) -> &'static DriverRow {
    let [audio, macos, library, config] = &REGISTRY;
    match driver {
        Driver::Audio => audio,
        Driver::Macos => macos,
        Driver::Library => library,
        Driver::Config => config,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::Driver;

    use crate::registry::{REGISTRY, row};

    #[test]
    fn every_driver_has_one_row() {
        assert_eq!(
            REGISTRY.map(|entry| entry.driver),
            [
                Driver::Audio,
                Driver::Macos,
                Driver::Library,
                Driver::Config
            ]
        );
        for driver in Driver::ALL {
            assert_eq!(row(driver).driver, driver);
        }
    }
}
