use audio::tap::SpectrumTap;
#[cfg(target_os = "macos")] use kernel::cmd::MacosCmd;
use kernel::cmd::{AudioCmd, ConfigCmd, LibraryCmd};
#[cfg(test)] use kernel::domain::driver::DriverName;

use crate::{
    driver_thread::DriverThread,
    error::Error,
    spawn::{
        audio_thread::spawn_audio,
        config_thread::spawn_config,
        library_thread::spawn_library,
    },
    spawn_setup::SpawnSetup,
};
#[cfg(test)]
use crate::{
    driver_thread::spawn_idle,
    registry,
    spawn::{audio_thread::idle_audio, library_thread::idle_library},
};

pub(crate) mod audio_thread;
pub(crate) mod config_thread;
mod library_thread;
pub(crate) mod macos_thread;

pub(crate) type Spawn<C> = fn(&SpawnSetup<'_>) -> Result<DriverThread<C>, Error>;

pub(crate) type SpawnAudio =
    fn(&SpawnSetup<'_>) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error>;

#[derive(Debug, Clone, Copy)]
pub struct Spawners {
    pub(crate) audio: SpawnAudio,
    pub(crate) library: Spawn<LibraryCmd>,
    pub(crate) config: Spawn<ConfigCmd>,
    #[cfg(target_os = "macos")]
    pub(crate) macos: Spawn<MacosCmd>,
}

impl Spawners {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn idle() -> Self {
        Self {
            audio: idle_audio,
            library: idle_library,
            config: |setup| spawn_idle(registry::row(DriverName::Config), setup.inbox),
            #[cfg(target_os = "macos")]
            macos: |setup| spawn_idle(registry::row(DriverName::Macos), setup.inbox),
        }
    }

    #[must_use]
    pub fn hardware() -> Self {
        Self {
            audio: spawn_audio,
            library: spawn_library,
            config: spawn_config,
            #[cfg(target_os = "macos")]
            macos: macos_thread::spawn,
        }
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use std::{
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use audio::tap::SpectrumTap;
    use config::driver::paths::{ConfigPaths, SeenTexts};
    use crossbeam_channel::{Receiver, Sender};
    use kernel::{
        cmd::AudioCmd,
        domain::{driver::DriverName, startup::Startup},
        message::Message,
    };
    use library::dirs::LibraryDirs;
    use rstest::rstest;

    use crate::{
        driver_thread::{Congestion, DriverThread, spawn_driver},
        error::Error,
        runtime::Runtime,
        spawn::{ConfigCmd, LibraryCmd, Spawners, audio_thread::idle_audio},
        spawn_setup::{SpawnSetup, StartupPaths},
    };

    pub(crate) fn spawn_audio_loop<R>(
        run: R,
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error>
    where
        R: FnOnce(&Receiver<AudioCmd>, &Sender<Message>, &Congestion) + Send + 'static,
    {
        let thread =
            spawn_driver(crate::registry::row(DriverName::Audio), run, setup.inbox)?;
        Ok((thread, SpectrumTap::silent()))
    }

    pub(crate) fn boom() -> ! {
        panic!("boom")
    }

    pub(crate) const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    pub(crate) fn stub_paths(directory: &Path) -> StartupPaths {
        StartupPaths {
            config: ConfigPaths {
                config: directory.join("config.toml"),
                appearance: directory.join("sifr-ui.toml"),
                themes: directory.join("themes"),
                default_music_dir: None,
                theme: None,
                seen: SeenTexts::default(),
            },
            library: LibraryDirs::under(directory),
        }
    }

    static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);
    #[cfg(target_os = "macos")]
    static MACOS_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counting_audio(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        AUDIO_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_audio(setup)
    }

    fn counting_library(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<LibraryCmd>, Error> {
        LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        (Spawners::idle().library)(setup)
    }

    fn counting_config(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<ConfigCmd>, Error> {
        CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        (Spawners::idle().config)(setup)
    }

    #[cfg(target_os = "macos")]
    fn counting_macos(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<kernel::cmd::MacosCmd>, Error> {
        MACOS_CALLS.fetch_add(1, Ordering::SeqCst);
        (Spawners::idle().macos)(setup)
    }

    #[test]
    fn every_driver_starts_through_its_spawner() {
        AUDIO_CALLS.store(0, Ordering::SeqCst);
        LIBRARY_CALLS.store(0, Ordering::SeqCst);
        CONFIG_CALLS.store(0, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        MACOS_CALLS.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: counting_audio,
            library: counting_library,
            config: counting_config,
            #[cfg(target_os = "macos")]
            macos: counting_macos,
        };

        let runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();
        runtime.drain();

        assert_eq!(AUDIO_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(LIBRARY_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(CONFIG_CALLS.load(Ordering::SeqCst), 1);
        #[cfg(target_os = "macos")]
        assert_eq!(MACOS_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_idle_spawner_never_opens_hardware() {
        let directory = tempfile::tempdir().unwrap();
        let paths = stub_paths(directory.path());
        let (inbox, _arrivals) = crossbeam_channel::bounded(4);
        let (model, _cmd) = kernel::update::startup::startup(Startup::default());
        let (writers, _cells, _notified) = crate::latest::latest_channels();
        let setup = SpawnSetup {
            audio: &model.settings.audio,
            paths: &paths,
            inbox: &inbox,
            writers: &writers,
            #[cfg(target_os = "macos")]
            macos: &crate::spawn_setup::MacosChannel::new(),
        };
        let (audio, _tap) = idle_audio(&setup).unwrap();

        drop(audio.commands);
        audio.handle.join().unwrap().unwrap();
    }

    static RESTART_LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESTART_CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn panicking_library(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<LibraryCmd>, Error> {
        RESTART_LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        spawn_driver(
            crate::registry::row(DriverName::Library),
            |_: &Receiver<LibraryCmd>, _: &Sender<Message>, _: &Congestion| boom(),
            setup.inbox,
        )
    }

    fn panicking_config(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<ConfigCmd>, Error> {
        RESTART_CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        spawn_driver(
            crate::registry::row(DriverName::Config),
            |_: &Receiver<ConfigCmd>, _: &Sender<Message>, _: &Congestion| boom(),
            setup.inbox,
        )
    }

    struct RestartRow {
        driver: DriverName,
        deaths: usize,
        spawns: usize,
        calls: &'static AtomicUsize,
    }

    fn spawners_for(driver: DriverName) -> Spawners {
        if driver == DriverName::Library {
            Spawners {
                library: panicking_library,
                ..Spawners::idle()
            }
        } else {
            Spawners {
                config: panicking_config,
                ..Spawners::idle()
            }
        }
    }

    #[rstest]
    #[case::config_degrades_without_a_restart(RestartRow {
        driver: DriverName::Config,
        deaths: 1,
        spawns: 1,
        calls: &RESTART_CONFIG_CALLS,
    })]
    #[case::library_restarts_once_then_degrades(RestartRow {
        driver: DriverName::Library,
        deaths: 2,
        spawns: 2,
        calls: &RESTART_LIBRARY_CALLS,
    })]
    fn restart_follows_the_row(#[case] row: RestartRow) {
        row.calls.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = spawners_for(row.driver);
        let mut runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();

        for _ in 0..row.deaths {
            let died = runtime.wiring.mailbox.recv_timeout(RECV_TIMEOUT).unwrap();
            runtime.deliver(died).unwrap();
        }

        assert_eq!(row.calls.load(Ordering::SeqCst), row.spawns);
        assert!(!runtime.model.workspace.toasts.is_empty());
        runtime.drain();
    }
}
