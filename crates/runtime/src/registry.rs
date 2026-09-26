use kernel::domain::{Driver, Supervision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
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
    pub(crate) thread: &'static str,
    pub(crate) placement: Placement,
    pub(crate) platform: Platform,
    pub(crate) inbox: usize,
    pub(crate) supervision: Supervision,
}

pub(crate) const REGISTRY: [DriverRow; 4] = [
    DriverRow {
        driver: Driver::Audio,
        thread: "sifr-audio",
        placement: Placement::Worker,
        platform: Platform::Every,
        inbox: 64,
        supervision: Supervision::standard(Driver::Audio),
    },
    DriverRow {
        driver: Driver::Macos,
        thread: "sifr-macos",
        placement: Placement::WorkerWithMainLoop,
        platform: Platform::Macos,
        inbox: 64,
        supervision: Supervision::standard(Driver::Macos),
    },
    DriverRow {
        driver: Driver::Library,
        thread: "sifr-library",
        placement: Placement::Worker,
        platform: Platform::Every,
        inbox: 64,
        supervision: Supervision::standard(Driver::Library),
    },
    DriverRow {
        driver: Driver::Config,
        thread: "sifr-config",
        placement: Placement::Worker,
        platform: Platform::Every,
        inbox: 64,
        supervision: Supervision::standard(Driver::Config),
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
    use rstest::rstest;

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

    #[rstest]
    #[case::audio(Driver::Audio)]
    #[case::library(Driver::Library)]
    #[case::config(Driver::Config)]
    #[case::macos(Driver::Macos)]
    fn supervision_defaults_match_the_kernel_standard(#[case] driver: Driver) {
        assert_eq!(
            row(driver).supervision,
            kernel::domain::Supervision::standard(driver)
        );
    }
}
