use audio::{DECODABLE_EXTENSIONS, SpectrumTap};
use crossbeam_channel::Sender;
use kernel::{
    AudioCmd,
    ConfigCmd,
    MacosCmd,
    Message,
    domain::{Driver, Model},
};

use crate::{
    cells::Senders,
    config::{ConfigPaths, ConfigTiming, driver::ConfigParts},
    driver::{DriverThread, NoDriver, spawn_loop},
    error::Error,
    library::{driver::LibraryParts, machine::LibraryMessage},
    registry,
    runtime::StartupPaths,
};

#[derive(Debug)]
pub(crate) struct SpawnParts<'a> {
    pub model: &'a Model,
    pub paths: &'a StartupPaths,
    pub sender: &'a Sender<Message>,
    pub(crate) writers: &'a Senders,
}

pub(crate) type AudioSpawned = (DriverThread<AudioCmd>, SpectrumTap);

pub(crate) type Spawn<C> = fn(&SpawnParts<'_>) -> Result<DriverThread<C>, Error>;

pub(crate) type SpawnAudio = fn(&SpawnParts<'_>) -> Result<AudioSpawned, Error>;

#[derive(Debug, Clone, Copy)]
pub struct Spawners {
    pub(crate) audio: SpawnAudio,
    pub(crate) library: Spawn<LibraryMessage>,
    pub(crate) config: Spawn<ConfigCmd>,
    pub(crate) macos: Spawn<MacosCmd>,
}

impl Spawners {
    #[must_use]
    pub fn idle() -> Self {
        Self {
            audio: idle_audio,
            library: idle_library,
            config: idle_config,
            macos: idle_macos,
        }
    }

    #[must_use]
    pub fn hardware() -> Self {
        Self {
            audio: spawn_audio,
            library: spawn_library,
            config: spawn_config,
            macos: spawn_macos,
        }
    }
}

#[cfg(test)]
pub(crate) fn spawn_audio_loop<L>(
    driver_loop: L,
    spawn_parts: &SpawnParts<'_>,
) -> Result<AudioSpawned, Error>
where
    L: crate::driver::DriverLoop<AudioCmd, kernel::AudioEvent>,
{
    let thread = spawn_loop(
        registry::row(Driver::Audio),
        driver_loop,
        spawn_parts.sender,
    )?;
    Ok((thread, SpectrumTap::silent()))
}

fn idle<C: Send + 'static>(
    driver: Driver,
    spawn_parts: &SpawnParts<'_>,
) -> Result<DriverThread<C>, Error> {
    spawn_loop::<C, Message, _>(registry::row(driver), NoDriver, spawn_parts.sender)
}

fn idle_audio(spawn_parts: &SpawnParts<'_>) -> Result<AudioSpawned, Error> {
    let thread = idle::<AudioCmd>(Driver::Audio, spawn_parts)?;
    Ok((thread, SpectrumTap::silent()))
}

fn idle_library(
    spawn_parts: &SpawnParts<'_>,
) -> Result<DriverThread<LibraryMessage>, Error> {
    idle::<LibraryMessage>(Driver::Library, spawn_parts)
}

fn idle_config(spawn_parts: &SpawnParts<'_>) -> Result<DriverThread<ConfigCmd>, Error> {
    idle::<ConfigCmd>(Driver::Config, spawn_parts)
}

fn idle_macos(spawn_parts: &SpawnParts<'_>) -> Result<DriverThread<MacosCmd>, Error> {
    idle::<MacosCmd>(Driver::Macos, spawn_parts)
}

fn spawn_audio(spawn_parts: &SpawnParts<'_>) -> Result<AudioSpawned, Error> {
    let (audio_loop, spectrum) = crate::audio::prepare(spawn_parts.model);
    let thread =
        spawn_loop(registry::row(Driver::Audio), audio_loop, spawn_parts.sender)?;
    Ok((thread, spectrum))
}

fn spawn_library(
    spawn_parts: &SpawnParts<'_>,
) -> Result<DriverThread<LibraryMessage>, Error> {
    let parts = LibraryParts {
        dirs: spawn_parts.paths.library.clone(),
        decodable: DECODABLE_EXTENSIONS,
    };
    crate::library::driver::spawn(
        parts,
        spawn_parts.sender,
        spawn_parts.writers.cover.clone(),
    )
}

pub(crate) fn spawn_config(
    spawn_parts: &SpawnParts<'_>,
) -> Result<DriverThread<ConfigCmd>, Error> {
    let config_paths = ConfigPaths {
        theme: Some(spawn_parts.model.themes.selected.to_string()),
        ..spawn_parts.paths.config.clone()
    };
    let parts = ConfigParts {
        paths: config_paths,
        timing: ConfigTiming::default(),
        theme: spawn_parts.writers.theme.clone(),
        appearance: spawn_parts.writers.appearance.clone(),
    };
    crate::config::driver::spawn(parts, spawn_parts.sender)
}

#[cfg(target_os = "macos")]
fn spawn_macos(spawn_parts: &SpawnParts<'_>) -> Result<DriverThread<MacosCmd>, Error> {
    let macos = crate::macos::MacosStart::new(library::embedded_cover);
    crate::macos::spawn(macos, spawn_parts.sender)
}

#[cfg(not(target_os = "macos"))]
fn spawn_macos(spawn_parts: &SpawnParts<'_>) -> Result<DriverThread<MacosCmd>, Error> {
    idle_macos(spawn_parts)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        AudioCmd,
        AudioEvent,
        Message,
        domain::{Direction, Driver, DriverStatus, SettingRow, Startup},
    };
    use library::LibraryDirs;
    use rstest::rstest;

    use crate::{
        config::ConfigPaths,
        driver::{DriverLoop, DriverThread},
        error::Error,
        runtime::{Runtime, StartupPaths},
        sender::DriverSender,
        spawn::{
            AudioSpawned,
            ConfigCmd,
            LibraryMessage,
            SpawnParts,
            Spawners,
            idle,
            idle_audio,
            idle_config,
            idle_library,
            idle_macos,
            spawn_audio_loop,
        },
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    fn stub_paths(directory: &Path) -> StartupPaths {
        StartupPaths {
            config: ConfigPaths {
                config: directory.join("config.toml"),
                appearance: directory.join("sifr-ui.toml"),
                themes: directory.join("themes"),
                theme: None,
                seen: crate::config::SeenTexts::default(),
            },
            library: LibraryDirs {
                cache_dir: directory.join("cache"),
                data_dir: directory.join("data"),
                playlists_dir: directory.join("playlists"),
            },
        }
    }

    static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);
    static MACOS_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counting_audio(spawn_parts: &SpawnParts<'_>) -> Result<AudioSpawned, Error> {
        AUDIO_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_audio(spawn_parts)
    }

    fn counting_library(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<LibraryMessage>, Error> {
        LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_library(spawn_parts)
    }

    fn counting_config(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<ConfigCmd>, Error> {
        CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_config(spawn_parts)
    }

    fn counting_macos(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<kernel::MacosCmd>, Error> {
        MACOS_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_macos(spawn_parts)
    }

    #[test]
    fn every_driver_starts_through_its_spawner() {
        AUDIO_CALLS.store(0, Ordering::SeqCst);
        LIBRARY_CALLS.store(0, Ordering::SeqCst);
        CONFIG_CALLS.store(0, Ordering::SeqCst);
        MACOS_CALLS.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: counting_audio,
            library: counting_library,
            config: counting_config,
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
        assert_eq!(MACOS_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_idle_spawner_never_opens_hardware() {
        let directory = tempfile::tempdir().unwrap();
        let paths = stub_paths(directory.path());
        let (sender, _arrivals) = crossbeam_channel::bounded(4);
        let (model, _cmd) = kernel::startup(Startup::default());
        let (writers, _cells, _notified) = crate::cells::cells();
        let spawn_parts = SpawnParts {
            model: &model,
            paths: &paths,
            sender: &sender,
            writers: &writers,
        };

        let audio = idle::<AudioCmd>(Driver::Audio, &spawn_parts).unwrap();

        drop(audio.commands);
        audio.handle.join().unwrap().unwrap();
    }

    fn died_from_replaygain_nudge(runtime: &mut Runtime) -> Message {
        runtime.step(Message::Adjust {
            row: SettingRow::Replaygain,
            direction: Direction::Next,
        });
        runtime.wiring.receiver.recv_timeout(RECV_TIMEOUT).unwrap()
    }

    static AUDIO_RESTART_SPAWNES: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static AUDIO_RESTART_FORWARD: RefCell<Option<Sender<AudioCmd>>> =
            const { RefCell::new(None) };
    }

    struct PanicOnceThenRecordAudio {
        forward: Sender<AudioCmd>,
    }

    impl DriverLoop<AudioCmd, AudioEvent> for PanicOnceThenRecordAudio {
        fn run(self, inbox: &Receiver<AudioCmd>, _outbox: &DriverSender<AudioEvent>) {
            if AUDIO_RESTART_SPAWNES.fetch_add(1, Ordering::SeqCst) == 0 {
                let _ = inbox.recv();
                panic!("boom");
            }
            while let Ok(command) = inbox.recv() {
                if self.forward.send(command).is_err() {
                    return;
                }
            }
        }
    }

    fn panic_once_then_record_audio(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<AudioSpawned, Error> {
        let forward = AUDIO_RESTART_FORWARD
            .with(|slot| slot.borrow().clone())
            .unwrap();
        spawn_audio_loop(PanicOnceThenRecordAudio { forward }, spawn_parts)
    }

    #[test]
    fn a_panicking_audio_driver_is_restarted_and_started() {
        AUDIO_RESTART_SPAWNES.store(0, Ordering::SeqCst);
        let (forward, commands) = unbounded();
        AUDIO_RESTART_FORWARD.with(|slot| *slot.borrow_mut() = Some(forward));
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: panic_once_then_record_audio,
            ..Spawners::idle()
        };
        let mut runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();

        let died = died_from_replaygain_nudge(&mut runtime);
        runtime.step(died);

        let received: Vec<AudioCmd> = (0..4)
            .map(|_| commands.recv_timeout(RECV_TIMEOUT).unwrap())
            .collect();
        assert!(matches!(received[0], AudioCmd::ListDevices));
        assert!(matches!(received[1], AudioCmd::SetDevice(_)));
        assert!(matches!(received[2], AudioCmd::SetCrossfade(_)));
        assert!(matches!(received[3], AudioCmd::SetReplaygain(_)));
        assert_eq!(
            *runtime.model.drivers.status(Driver::Audio),
            DriverStatus::Running
        );
        runtime.drain();
    }

    static DROP_SPAWN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    static FIRST_DROP_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
    static SECOND_SPAWN_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
    static SEQUENCED_SPAWNES: AtomicUsize = AtomicUsize::new(0);

    struct SequencedAudio;

    impl Drop for SequencedAudio {
        fn drop(&mut self) {
            let order = DROP_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            FIRST_DROP_ORDER.store(order, Ordering::SeqCst);
        }
    }

    impl DriverLoop<AudioCmd, AudioEvent> for SequencedAudio {
        fn run(self, inbox: &Receiver<AudioCmd>, _outbox: &DriverSender<AudioEvent>) {
            let _ = inbox.recv();
            panic!("boom");
        }
    }

    fn sequenced_audio(spawn_parts: &SpawnParts<'_>) -> Result<AudioSpawned, Error> {
        if SEQUENCED_SPAWNES.fetch_add(1, Ordering::SeqCst) == 1 {
            let order = DROP_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            SECOND_SPAWN_ORDER.store(order, Ordering::SeqCst);
        }
        spawn_audio_loop(SequencedAudio, spawn_parts)
    }

    #[test]
    fn the_dead_thread_is_joined_before_the_new_one_spawns() {
        DROP_SPAWN_SEQUENCE.store(0, Ordering::SeqCst);
        FIRST_DROP_ORDER.store(usize::MAX, Ordering::SeqCst);
        SECOND_SPAWN_ORDER.store(usize::MAX, Ordering::SeqCst);
        SEQUENCED_SPAWNES.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: sequenced_audio,
            ..Spawners::idle()
        };
        let mut runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();

        let died = died_from_replaygain_nudge(&mut runtime);
        runtime.step(died);

        assert!(
            FIRST_DROP_ORDER.load(Ordering::SeqCst)
                < SECOND_SPAWN_ORDER.load(Ordering::SeqCst)
        );
        runtime.drain();
    }

    static RESTART_LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESTART_CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);

    struct PanicOnStart;

    impl<C: Send + 'static, F: Send + 'static> DriverLoop<C, F> for PanicOnStart {
        fn run(self, _inbox: &Receiver<C>, _outbox: &DriverSender<F>) {
            panic!("boom");
        }
    }

    fn panicking_library(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<LibraryMessage>, Error> {
        RESTART_LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        crate::driver::spawn_loop::<LibraryMessage, Message, _>(
            crate::registry::row(Driver::Library),
            PanicOnStart,
            spawn_parts.sender,
        )
    }

    fn panicking_config(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<ConfigCmd>, Error> {
        RESTART_CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        crate::driver::spawn_loop::<ConfigCmd, Message, _>(
            crate::registry::row(Driver::Config),
            PanicOnStart,
            spawn_parts.sender,
        )
    }

    struct RestartRow {
        driver: Driver,
        deaths: usize,
        spawns: usize,
        calls: &'static AtomicUsize,
    }

    fn spawners_for(driver: Driver) -> Spawners {
        if driver == Driver::Library {
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
        driver: Driver::Config,
        deaths: 1,
        spawns: 1,
        calls: &RESTART_CONFIG_CALLS,
    })]
    #[case::library_restarts_once_then_degrades(RestartRow {
        driver: Driver::Library,
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
            let died = runtime.wiring.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
            runtime.step(died);
        }

        assert_eq!(row.calls.load(Ordering::SeqCst), row.spawns);
        assert!(runtime.model.workspace.toast.is_some());
        runtime.drain();
    }
}
