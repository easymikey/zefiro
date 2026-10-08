use audio::tap::SpectrumTap;
#[cfg(target_os = "macos")] use kernel::cmd::MacosCmd;
use kernel::cmd::{AudioCmd, ConfigCmd, LibraryCmd, RemoteCmd};

use crate::{
    driver_thread::DriverThread,
    error::SpawnError,
    spawn::{
        audio_thread::spawn_audio,
        config_thread::spawn_config,
        library_thread::spawn_library,
        remote_thread::spawn_remote,
    },
    spawn_setup::SpawnSetup,
};

pub(crate) mod audio_thread;
pub(crate) mod config_thread;
mod library_thread;
pub(crate) mod macos_thread;
mod remote_thread;

pub(crate) type Spawner<C> = fn(&SpawnSetup<'_>) -> Result<DriverThread<C>, SpawnError>;

pub(crate) type SpawnAudio =
    fn(&SpawnSetup<'_>) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError>;

#[derive(Debug, Clone, Copy)]
pub struct Spawners {
    pub(crate) audio: SpawnAudio,
    pub(crate) library: Spawner<LibraryCmd>,
    pub(crate) config: Spawner<ConfigCmd>,
    #[cfg(target_os = "macos")]
    pub(crate) macos: Spawner<MacosCmd>,
    pub(crate) remote: Spawner<RemoteCmd>,
}

impl Spawners {
    #[must_use]
    pub fn hardware() -> Self {
        Self {
            audio: spawn_audio,
            library: spawn_library,
            config: spawn_config,
            #[cfg(target_os = "macos")]
            macos: macos_thread::spawn_macos,
            remote: spawn_remote,
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
        domain::{
            driver::{DriverError, DriverName},
            startup::Startup,
        },
        message::Message,
    };
    use library::dirs::LibraryDirs;
    use rstest::rstest;

    use crate::{
        driver_thread::{Congestion, DriverThread, spawn_driver},
        error::SpawnError,
        latest::LatestReceivers,
        registry::{self, DriverRow},
        runtime::Runtime,
        spawn::{ConfigCmd, LibraryCmd, RemoteCmd, Spawners},
        spawn_setup::{SpawnSetup, StartupPaths},
    };

    pub(crate) fn spawn_idle<C: Send + 'static>(
        row: &DriverRow,
        inbox: &Sender<Message>,
    ) -> Result<DriverThread<C>, SpawnError> {
        spawn_driver(
            row,
            |cmd_receiver: &Receiver<C>, _: &Sender<Message>, _: &Congestion| {
                while cmd_receiver.recv().is_ok() {}
                Ok(())
            },
            inbox,
        )
    }

    pub(crate) fn idle_audio(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        let thread = spawn_idle(registry::row(DriverName::Audio), setup.inbox)?;
        Ok((thread, SpectrumTap::silent()))
    }

    pub(crate) fn idle_library(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<LibraryCmd>, SpawnError> {
        spawn_idle(registry::row(DriverName::Library), setup.inbox)
    }

    pub(crate) fn idle_spawners() -> Spawners {
        Spawners {
            audio: idle_audio,
            library: idle_library,
            config: |setup| spawn_idle(registry::row(DriverName::Config), setup.inbox),
            #[cfg(target_os = "macos")]
            macos: |setup| spawn_idle(registry::row(DriverName::Macos), setup.inbox),
            remote: |setup| spawn_idle(registry::row(DriverName::Remote), setup.inbox),
        }
    }

    pub(crate) fn spawn_audio_loop<R>(
        run: R,
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError>
    where
        R: FnOnce(
                &Receiver<AudioCmd>,
                &Sender<Message>,
                &Congestion,
            ) -> Result<(), DriverError>
            + Send
            + 'static,
    {
        let thread = spawn_driver(registry::row(DriverName::Audio), run, setup.inbox)?;
        Ok((thread, SpectrumTap::silent()))
    }

    pub(crate) fn boom() -> ! {
        panic!("boom")
    }

    pub(crate) const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    pub(crate) const SETTLE_TIMEOUT: Duration = Duration::from_millis(200);

    pub(crate) fn drain<T>(receiver: &Receiver<T>) -> Vec<T> {
        let mut collected = vec![receiver.recv_timeout(RECV_TIMEOUT).unwrap()];
        collected.extend(std::iter::from_fn(|| {
            receiver.recv_timeout(SETTLE_TIMEOUT).ok()
        }));
        collected
    }

    pub(crate) fn spawned_with<T>(
        spawner: fn(&SpawnSetup<'_>) -> Result<T, SpawnError>,
        paths: &StartupPaths,
        inbox: &Sender<Message>,
    ) -> (T, LatestReceivers, Receiver<()>) {
        let (model, _cmd) = kernel::update::startup::startup(Startup::default());
        let (latest_senders, latest_receivers, doorbell) =
            crate::latest::latest_channels();
        let spawned = spawner(&SpawnSetup {
            audio_settings: &model.settings.audio_settings,
            paths,
            inbox,
            latest_senders: &latest_senders,
            #[cfg(target_os = "macos")]
            macos_channel: &crate::spawn_setup::MacosChannel::new(),
        })
        .unwrap();
        (spawned, latest_receivers, doorbell)
    }

    pub(crate) fn stub_paths(dir: &Path) -> StartupPaths {
        StartupPaths {
            config_paths: ConfigPaths {
                config_path: dir.join("config.toml"),
                appearance_path: dir.join("sifr-ui.toml"),
                themes_dir: dir.join("themes"),
                default_music_dir: None,
                theme_name: None,
                seen_texts: SeenTexts::default(),
            },
            library_dirs: LibraryDirs::new(
                &dir.join("cache"),
                &dir.join("data"),
                &dir.join("config"),
            ),
        }
    }

    static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);
    #[cfg(target_os = "macos")]
    static MACOS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static REMOTE_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counting_audio(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        AUDIO_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_audio(setup)
    }

    fn counting_library(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<LibraryCmd>, SpawnError> {
        LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_library(setup)
    }

    fn counting_config(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<ConfigCmd>, SpawnError> {
        CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        (idle_spawners().config)(setup)
    }

    #[cfg(target_os = "macos")]
    fn counting_macos(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<kernel::cmd::MacosCmd>, SpawnError> {
        MACOS_CALLS.fetch_add(1, Ordering::SeqCst);
        (idle_spawners().macos)(setup)
    }

    fn counting_remote(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<RemoteCmd>, SpawnError> {
        REMOTE_CALLS.fetch_add(1, Ordering::SeqCst);
        (idle_spawners().remote)(setup)
    }

    #[test]
    fn every_driver_starts_through_its_spawner() {
        AUDIO_CALLS.store(0, Ordering::SeqCst);
        LIBRARY_CALLS.store(0, Ordering::SeqCst);
        CONFIG_CALLS.store(0, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        MACOS_CALLS.store(0, Ordering::SeqCst);
        REMOTE_CALLS.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: counting_audio,
            library: counting_library,
            config: counting_config,
            #[cfg(target_os = "macos")]
            macos: counting_macos,
            remote: counting_remote,
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
        assert_eq!(REMOTE_CALLS.load(Ordering::SeqCst), 1);
    }

    static RESTART_LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESTART_CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn panicking_library(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<LibraryCmd>, SpawnError> {
        RESTART_LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        spawn_driver(
            registry::row(DriverName::Library),
            |_: &Receiver<LibraryCmd>, _: &Sender<Message>, _: &Congestion| boom(),
            setup.inbox,
        )
    }

    fn panicking_config(
        setup: &SpawnSetup<'_>,
    ) -> Result<DriverThread<ConfigCmd>, SpawnError> {
        RESTART_CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        spawn_driver(
            registry::row(DriverName::Config),
            |_: &Receiver<ConfigCmd>, _: &Sender<Message>, _: &Congestion| boom(),
            setup.inbox,
        )
    }

    struct RestartRow {
        driver_name: DriverName,
        deaths: usize,
        spawns: usize,
        calls: &'static AtomicUsize,
    }

    fn spawners_for(driver_name: DriverName) -> Spawners {
        if driver_name == DriverName::Library {
            Spawners {
                library: panicking_library,
                ..idle_spawners()
            }
        } else {
            Spawners {
                config: panicking_config,
                ..idle_spawners()
            }
        }
    }

    #[rstest]
    #[case::config_degrades_without_a_restart(RestartRow {
        driver_name: DriverName::Config,
        deaths: 1,
        spawns: 1,
        calls: &RESTART_CONFIG_CALLS,
    })]
    #[case::library_restarts_once_then_degrades(RestartRow {
        driver_name: DriverName::Library,
        deaths: 2,
        spawns: 2,
        calls: &RESTART_LIBRARY_CALLS,
    })]
    fn restart_follows_the_row(#[case] row: RestartRow) {
        row.calls.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = spawners_for(row.driver_name);
        let mut runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();

        for _ in 0..row.deaths {
            let died = runtime
                .wiring
                .inbox_receiver
                .recv_timeout(RECV_TIMEOUT)
                .unwrap();
            runtime.deliver(died).unwrap();
        }

        assert_eq!(row.calls.load(Ordering::SeqCst), row.spawns);
        assert!(!runtime.model.workspace.toasts.is_empty());
        runtime.drain();
    }
}
