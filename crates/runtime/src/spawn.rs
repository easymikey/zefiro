use audio::{DECODABLE_EXTENSIONS, SpectrumTap};
use crossbeam_channel::Sender;
use kernel::{
    AudioCmd,
    ConfigCmd,
    MacosCmd,
    Message,
    domain::{DriverName, Model},
};

use crate::{
    config::{ConfigPaths, driver::ConfigParts, machine::SAVE_DEBOUNCE},
    driver::{DriverLoop, DriverThread, spawn_idle},
    error::Error,
    jobs::Jobs,
    latest::LatestSenders,
    library::{driver::LibraryParts, machine::LibraryMessage},
    registry,
    runtime::StartupPaths,
};

#[derive(Debug)]
pub(crate) struct SpawnParts<'a> {
    pub(crate) model: &'a Model,
    pub(crate) paths: &'a StartupPaths,
    pub(crate) sender: &'a Sender<Message>,
    pub(crate) writers: &'a LatestSenders,
}

pub(crate) type Spawn<C> = fn(&SpawnParts<'_>) -> Result<DriverThread<C>, Error>;

pub(crate) type StartAudio =
    fn(&SpawnParts<'_>) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error>;

#[derive(Debug, Clone, Copy)]
pub struct Spawners {
    pub(crate) audio: StartAudio,
    pub(crate) library: Spawn<LibraryMessage>,
    pub(crate) config: Spawn<ConfigCmd>,
    pub(crate) macos: Spawn<MacosCmd>,
}

impl Spawners {
    #[must_use]
    pub fn idle() -> Self {
        Self {
            audio: idle_audio,
            library: |parts| {
                spawn_idle(registry::row(DriverName::Library), parts.sender)
            },
            config: |parts| spawn_idle(registry::row(DriverName::Config), parts.sender),
            macos: |parts| spawn_idle(registry::row(DriverName::Macos), parts.sender),
        }
    }

    #[must_use]
    pub fn hardware() -> Self {
        Self {
            audio: spawn_audio,
            library: spawn_library,
            config: spawn_config,
            #[cfg(target_os = "macos")]
            macos: spawn_macos,
            #[cfg(not(target_os = "macos"))]
            macos: Self::idle().macos,
        }
    }
}

fn idle_audio(
    spawn_parts: &SpawnParts<'_>,
) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
    let thread = spawn_idle(registry::row(DriverName::Audio), spawn_parts.sender)?;
    Ok((thread, SpectrumTap::silent()))
}

fn spawn_audio(
    spawn_parts: &SpawnParts<'_>,
) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
    let settings = spawn_parts.model.settings.audio.clone();
    let (tap_sender, tap_receiver) = crossbeam_channel::bounded(1);
    let (deck_sender, heard) = crossbeam_channel::bounded(64);
    let row = registry::row(DriverName::Audio);
    let jobs = Jobs {
        pick: audio::EngineEffect::into_job,
        run: audio::AudioJob::run,
    };
    let thread = DriverLoop::<audio::AudioDriver, _> {
        row,
        inbox: spawn_parts.sender.clone(),
        heard,
        jobs,
    }
    .spawn(move || {
        let (driver, spectrum) = audio::AudioDriver::new(settings, deck_sender);
        if let Err(unclaimed) = tap_sender.send(spectrum) {
            drop(unclaimed.into_inner());
        }
        driver
    })?;
    let spectrum = tap_receiver.recv().map_err(|_disconnected| Error::Spawn {
        driver: row.driver,
        source: std::io::Error::other(
            "audio driver stopped before handing over its tap",
        ),
    })?;
    Ok((thread, spectrum))
}

fn spawn_library(
    spawn_parts: &SpawnParts<'_>,
) -> Result<DriverThread<LibraryMessage>, Error> {
    let parts = LibraryParts::new(
        spawn_parts.paths.library.clone(),
        DECODABLE_EXTENSIONS,
        spawn_parts.writers.cover.clone(),
    )?;
    crate::library::driver::spawn(parts, spawn_parts.sender)
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
        save_debounce: SAVE_DEBOUNCE,
        theme: spawn_parts.writers.theme.clone(),
    };
    crate::config::driver::spawn(parts, spawn_parts.sender)
}

#[cfg(target_os = "macos")]
fn spawn_macos(spawn_parts: &SpawnParts<'_>) -> Result<DriverThread<MacosCmd>, Error> {
    crate::macos::spawn(library::embedded_cover, spawn_parts.sender)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        cell::RefCell,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use audio::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        AudioCmd,
        AudioEvent,
        Message,
        Outbox,
        domain::{Direction, DriverName, DriverStatus, SettingRow, Startup},
    };
    use library::LibraryDirs;
    use rstest::rstest;

    use crate::{
        config::ConfigPaths,
        driver::{DriverThread, spawn_driver},
        error::Error,
        runtime::{Runtime, StartupPaths},
        spawn::{ConfigCmd, LibraryMessage, SpawnParts, Spawners, idle_audio},
    };

    pub(crate) fn spawn_audio_loop<R>(
        run: R,
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error>
    where
        R: FnOnce(&Receiver<AudioCmd>, &Outbox<AudioEvent>) + Send + 'static,
    {
        let thread = spawn_driver(
            crate::registry::row(DriverName::Audio),
            run,
            spawn_parts.sender,
        )?;
        Ok((thread, SpectrumTap::silent()))
    }

    fn boom() -> ! {
        panic!("boom")
    }

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
            library: LibraryDirs::under(directory),
        }
    }

    static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);
    static MACOS_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counting_audio(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        AUDIO_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_audio(spawn_parts)
    }

    fn counting_library(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<LibraryMessage>, Error> {
        LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        (Spawners::idle().library)(spawn_parts)
    }

    fn counting_config(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<ConfigCmd>, Error> {
        CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        (Spawners::idle().config)(spawn_parts)
    }

    fn counting_macos(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<kernel::MacosCmd>, Error> {
        MACOS_CALLS.fetch_add(1, Ordering::SeqCst);
        (Spawners::idle().macos)(spawn_parts)
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
        let (writers, _cells, _notified) = crate::latest::latest_channels();
        let spawn_parts = SpawnParts {
            model: &model,
            paths: &paths,
            sender: &sender,
            writers: &writers,
        };

        let (audio, _tap) = idle_audio(&spawn_parts).unwrap();

        drop(audio.commands);
        audio.handle.join().unwrap().unwrap();
    }

    fn died_from_replay_gain_step(runtime: &mut Runtime) -> Message {
        runtime.step(Message::Step {
            row: SettingRow::ReplayGain,
            direction: Direction::Next,
        });
        runtime.wiring.receiver.recv_timeout(RECV_TIMEOUT).unwrap()
    }

    static AUDIO_RESTART_SPAWNES: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static AUDIO_RESTART_FORWARD: RefCell<Option<Sender<AudioCmd>>> =
            const { RefCell::new(None) };
    }

    fn panic_once_then_record_audio(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        let forward = AUDIO_RESTART_FORWARD
            .with(|slot| slot.borrow().clone())
            .unwrap();
        spawn_audio_loop(
            move |inbox: &Receiver<AudioCmd>, _: &Outbox<AudioEvent>| {
                if AUDIO_RESTART_SPAWNES.fetch_add(1, Ordering::SeqCst) == 0 {
                    let _ = inbox.recv();
                    boom();
                }
                while let Ok(command) = inbox.recv() {
                    if forward.send(command).is_err() {
                        return;
                    }
                }
            },
            spawn_parts,
        )
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

        let died = died_from_replay_gain_step(&mut runtime);
        runtime.step(died);

        let received: Vec<AudioCmd> = (0..4)
            .map(|_| commands.recv_timeout(RECV_TIMEOUT).unwrap())
            .collect();
        assert!(matches!(received[0], AudioCmd::ListDevices));
        assert!(matches!(received[1], AudioCmd::SetDevice(_)));
        assert!(matches!(received[2], AudioCmd::SetCrossfade(_)));
        assert!(matches!(received[3], AudioCmd::SetReplayGain(_)));
        assert_eq!(
            *runtime.model.drivers.status(DriverName::Audio),
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

    fn sequenced_audio(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        if SEQUENCED_SPAWNES.fetch_add(1, Ordering::SeqCst) == 1 {
            let order = DROP_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            SECOND_SPAWN_ORDER.store(order, Ordering::SeqCst);
        }
        spawn_audio_loop(
            |inbox: &Receiver<AudioCmd>, _: &Outbox<AudioEvent>| {
                let _sequenced = SequencedAudio;
                let _ = inbox.recv();
                boom();
            },
            spawn_parts,
        )
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

        let died = died_from_replay_gain_step(&mut runtime);
        runtime.step(died);

        assert!(
            FIRST_DROP_ORDER.load(Ordering::SeqCst)
                < SECOND_SPAWN_ORDER.load(Ordering::SeqCst)
        );
        runtime.drain();
    }

    static RESTART_LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESTART_CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn panicking_library(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<LibraryMessage>, Error> {
        RESTART_LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        spawn_driver(
            crate::registry::row(DriverName::Library),
            |_: &Receiver<LibraryMessage>, _: &Outbox<Message>| boom(),
            spawn_parts.sender,
        )
    }

    fn panicking_config(
        spawn_parts: &SpawnParts<'_>,
    ) -> Result<DriverThread<ConfigCmd>, Error> {
        RESTART_CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        spawn_driver(
            crate::registry::row(DriverName::Config),
            |_: &Receiver<ConfigCmd>, _: &Outbox<Message>| boom(),
            spawn_parts.sender,
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
            let died = runtime.wiring.receiver.recv_timeout(RECV_TIMEOUT).unwrap();
            runtime.step(died);
        }

        assert_eq!(row.calls.load(Ordering::SeqCst), row.spawns);
        assert!(!runtime.model.workspace.toasts.is_empty());
        runtime.drain();
    }
}
