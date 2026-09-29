use audio::{DECODABLE_EXTENSIONS, SpectrumTap};
use crossbeam_channel::Sender;
use kernel::{
    AudioCmd,
    Message,
    SystemCmd,
    domain::{Driver, Model},
};

use crate::{
    cells::Writers,
    config::{ConfigPaths, ConfigTiming, driver::spawn as spawn_config},
    driver::{DriverThread, NoDriver, spawn_loop},
    error::RuntimeError,
    interpret::{ConfigCommand, LibraryCommand},
    library::driver::spawn as spawn_library,
    registry,
    runtime::BootPaths,
};

#[derive(Debug)]
pub(crate) struct Launching<'a> {
    pub model: &'a Model,
    pub paths: &'a BootPaths,
    pub mailbox: &'a Sender<Message>,
    pub(crate) writers: &'a Writers,
}

#[derive(Debug)]
pub(crate) struct Launched<C> {
    pub(crate) thread: DriverThread<C>,
    pub(crate) spectrum: Option<SpectrumTap>,
}

pub(crate) type Launch<C> = fn(&Launching<'_>) -> Result<Launched<C>, RuntimeError>;

#[derive(Debug, Clone, Copy)]
pub struct Launchers {
    pub(crate) audio: Launch<AudioCmd>,
    pub(crate) library: Launch<LibraryCommand>,
    pub(crate) config: Launch<ConfigCommand>,
    pub(crate) macos: Launch<SystemCmd>,
}

impl Launchers {
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
    pub fn system() -> Self {
        Self {
            audio: launch_audio,
            library: launch_library,
            config: launch_config,
            macos: system_macos,
        }
    }
}

#[cfg(test)]
pub(crate) fn spawn_audio_launched<L>(
    driver_loop: L,
    launching: &Launching<'_>,
) -> Result<Launched<AudioCmd>, RuntimeError>
where
    L: crate::driver::DriverLoop<AudioCmd, kernel::AudioEvent>,
{
    let thread =
        spawn_loop(registry::row(Driver::Audio), driver_loop, launching.mailbox)?;
    Ok(Launched {
        thread,
        spectrum: Some(SpectrumTap::silent()),
    })
}

fn idle<C: Send + 'static>(
    driver: Driver,
    launching: &Launching<'_>,
) -> Result<Launched<C>, RuntimeError> {
    let thread = spawn_loop::<C, Message, _>(
        registry::row(driver),
        NoDriver,
        launching.mailbox,
    )?;
    Ok(Launched {
        thread,
        spectrum: None,
    })
}

fn idle_audio(launching: &Launching<'_>) -> Result<Launched<AudioCmd>, RuntimeError> {
    let mut launched = idle::<AudioCmd>(Driver::Audio, launching)?;
    launched.spectrum = Some(SpectrumTap::silent());
    Ok(launched)
}

fn idle_library(
    launching: &Launching<'_>,
) -> Result<Launched<LibraryCommand>, RuntimeError> {
    idle::<LibraryCommand>(Driver::Library, launching)
}

fn idle_config(
    launching: &Launching<'_>,
) -> Result<Launched<ConfigCommand>, RuntimeError> {
    idle::<ConfigCommand>(Driver::Config, launching)
}

fn idle_macos(launching: &Launching<'_>) -> Result<Launched<SystemCmd>, RuntimeError> {
    idle::<SystemCmd>(Driver::Macos, launching)
}

fn launch_audio(launching: &Launching<'_>) -> Result<Launched<AudioCmd>, RuntimeError> {
    let (audio_loop, spectrum) = crate::audio::prepare(launching.model);
    let thread =
        spawn_loop(registry::row(Driver::Audio), audio_loop, launching.mailbox)?;
    Ok(Launched {
        thread,
        spectrum: Some(spectrum),
    })
}

fn launch_library(
    launching: &Launching<'_>,
) -> Result<Launched<LibraryCommand>, RuntimeError> {
    let thread = spawn_library(
        (launching.paths.library.clone(), DECODABLE_EXTENSIONS),
        launching.mailbox,
        launching.writers.cover.clone(),
    )?;
    Ok(Launched {
        thread,
        spectrum: None,
    })
}

pub(crate) fn launch_config(
    launching: &Launching<'_>,
) -> Result<Launched<ConfigCommand>, RuntimeError> {
    let config_paths = ConfigPaths {
        theme: Some(launching.model.themes.selected.to_string()),
        ..launching.paths.config.clone()
    };
    let thread = spawn_config(
        (config_paths, ConfigTiming::default()),
        launching.mailbox,
        (
            launching.writers.theme.clone(),
            launching.writers.appearance.clone(),
        ),
    )?;
    Ok(Launched {
        thread,
        spectrum: None,
    })
}

#[cfg(target_os = "macos")]
fn system_macos(
    launching: &Launching<'_>,
) -> Result<Launched<SystemCmd>, RuntimeError> {
    let system = crate::macos::SystemStart::new(library::embedded_cover);
    let thread = crate::macos::spawn(system, launching.mailbox)?;
    Ok(Launched {
        thread,
        spectrum: None,
    })
}

#[cfg(not(target_os = "macos"))]
fn system_macos(
    launching: &Launching<'_>,
) -> Result<Launched<SystemCmd>, RuntimeError> {
    idle_macos(launching)
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
        domain::{Driver, DriverStatus, Nudge, SettingRow, Startup},
    };
    use library::LibraryPaths;
    use rstest::rstest;

    use crate::{
        config::ConfigPaths,
        driver::DriverLoop,
        error::RuntimeError,
        launch::{
            ConfigCommand,
            Launched,
            Launchers,
            Launching,
            LibraryCommand,
            idle,
            idle_audio,
            idle_config,
            idle_library,
            idle_macos,
            spawn_audio_launched,
        },
        mailbox::Mailbox,
        runtime::{BootPaths, Runtime},
    };

    const RECV_TIMEOUT: Duration = Duration::from_secs(1);

    fn stub_paths(directory: &Path) -> BootPaths {
        BootPaths {
            config: ConfigPaths {
                config: Some(directory.join("config.toml")),
                appearance: directory.join("sifr-ui.toml"),
                themes: directory.join("themes"),
                theme: None,
                seen: crate::config::SeenTexts::default(),
            },
            library: LibraryPaths {
                cache: directory.join("cache"),
                data: directory.join("data"),
                playlists: directory.join("playlists"),
            },
        }
    }

    static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);
    static MACOS_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counting_audio(
        launching: &Launching<'_>,
    ) -> Result<Launched<AudioCmd>, RuntimeError> {
        AUDIO_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_audio(launching)
    }

    fn counting_library(
        launching: &Launching<'_>,
    ) -> Result<Launched<LibraryCommand>, RuntimeError> {
        LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_library(launching)
    }

    fn counting_config(
        launching: &Launching<'_>,
    ) -> Result<Launched<ConfigCommand>, RuntimeError> {
        CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_config(launching)
    }

    fn counting_macos(
        launching: &Launching<'_>,
    ) -> Result<Launched<kernel::SystemCmd>, RuntimeError> {
        MACOS_CALLS.fetch_add(1, Ordering::SeqCst);
        idle_macos(launching)
    }

    #[test]
    fn every_driver_boots_through_its_launcher() {
        AUDIO_CALLS.store(0, Ordering::SeqCst);
        LIBRARY_CALLS.store(0, Ordering::SeqCst);
        CONFIG_CALLS.store(0, Ordering::SeqCst);
        MACOS_CALLS.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let launchers = Launchers {
            audio: counting_audio,
            library: counting_library,
            config: counting_config,
            macos: counting_macos,
        };

        let runtime = Runtime::boot(
            Startup::default(),
            &stub_paths(directory.path()),
            &launchers,
        )
        .unwrap();
        runtime.drain();

        assert_eq!(AUDIO_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(LIBRARY_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(CONFIG_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(MACOS_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_idle_launcher_never_opens_hardware() {
        let directory = tempfile::tempdir().unwrap();
        let paths = stub_paths(directory.path());
        let (mailbox, _arrivals) = crossbeam_channel::bounded(4);
        let (model, _cmd) = kernel::startup(Startup::default());
        let (writers, _cells, _doorbell) = crate::cells::cells();
        let launching = Launching {
            model: &model,
            paths: &paths,
            mailbox: &mailbox,
            writers: &writers,
        };

        let audio = idle::<AudioCmd>(Driver::Audio, &launching).unwrap();

        drop(audio.thread.commands);
        audio.thread.handle.join().unwrap().unwrap();
    }

    fn died_from_replaygain_nudge(runtime: &mut Runtime) -> Message {
        runtime.step(Message::Adjust {
            row: SettingRow::Replaygain,
            nudge: Nudge::Up,
        });
        runtime.wiring.mailbox.recv_timeout(RECV_TIMEOUT).unwrap()
    }

    static AUDIO_RESTART_LAUNCHES: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static AUDIO_RESTART_FORWARD: RefCell<Option<Sender<AudioCmd>>> =
            const { RefCell::new(None) };
    }

    struct PanicOnceThenRecordAudio {
        forward: Sender<AudioCmd>,
    }

    impl DriverLoop<AudioCmd, AudioEvent> for PanicOnceThenRecordAudio {
        fn run(self, inbox: &Receiver<AudioCmd>, _outbox: &Mailbox<AudioEvent>) {
            if AUDIO_RESTART_LAUNCHES.fetch_add(1, Ordering::SeqCst) == 0 {
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
        launching: &Launching<'_>,
    ) -> Result<Launched<AudioCmd>, RuntimeError> {
        let forward = AUDIO_RESTART_FORWARD
            .with(|slot| slot.borrow().clone())
            .unwrap();
        spawn_audio_launched(PanicOnceThenRecordAudio { forward }, launching)
    }

    #[test]
    fn a_panicking_audio_driver_is_relaunched_and_booted() {
        AUDIO_RESTART_LAUNCHES.store(0, Ordering::SeqCst);
        let (forward, commands) = unbounded();
        AUDIO_RESTART_FORWARD.with(|slot| *slot.borrow_mut() = Some(forward));
        let directory = tempfile::tempdir().unwrap();
        let launchers = Launchers {
            audio: panic_once_then_record_audio,
            ..Launchers::idle()
        };
        let mut runtime = Runtime::boot(
            Startup::default(),
            &stub_paths(directory.path()),
            &launchers,
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

    static DROP_LAUNCH_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    static FIRST_DROP_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
    static SECOND_LAUNCH_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
    static SEQUENCED_LAUNCHES: AtomicUsize = AtomicUsize::new(0);

    struct SequencedAudio;

    impl Drop for SequencedAudio {
        fn drop(&mut self) {
            let order = DROP_LAUNCH_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            FIRST_DROP_ORDER.store(order, Ordering::SeqCst);
        }
    }

    impl DriverLoop<AudioCmd, AudioEvent> for SequencedAudio {
        fn run(self, inbox: &Receiver<AudioCmd>, _outbox: &Mailbox<AudioEvent>) {
            let _ = inbox.recv();
            panic!("boom");
        }
    }

    fn sequenced_audio(
        launching: &Launching<'_>,
    ) -> Result<Launched<AudioCmd>, RuntimeError> {
        if SEQUENCED_LAUNCHES.fetch_add(1, Ordering::SeqCst) == 1 {
            let order = DROP_LAUNCH_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            SECOND_LAUNCH_ORDER.store(order, Ordering::SeqCst);
        }
        spawn_audio_launched(SequencedAudio, launching)
    }

    #[test]
    fn the_dead_thread_is_joined_before_the_new_one_launches() {
        DROP_LAUNCH_SEQUENCE.store(0, Ordering::SeqCst);
        FIRST_DROP_ORDER.store(usize::MAX, Ordering::SeqCst);
        SECOND_LAUNCH_ORDER.store(usize::MAX, Ordering::SeqCst);
        SEQUENCED_LAUNCHES.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let launchers = Launchers {
            audio: sequenced_audio,
            ..Launchers::idle()
        };
        let mut runtime = Runtime::boot(
            Startup::default(),
            &stub_paths(directory.path()),
            &launchers,
        )
        .unwrap();

        let died = died_from_replaygain_nudge(&mut runtime);
        runtime.step(died);

        assert!(
            FIRST_DROP_ORDER.load(Ordering::SeqCst)
                < SECOND_LAUNCH_ORDER.load(Ordering::SeqCst)
        );
        runtime.drain();
    }

    static RESTART_LIBRARY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESTART_CONFIG_CALLS: AtomicUsize = AtomicUsize::new(0);

    struct PanicOnBoot;

    impl<C: Send + 'static, F: Send + 'static> DriverLoop<C, F> for PanicOnBoot {
        fn run(self, _inbox: &Receiver<C>, _outbox: &Mailbox<F>) {
            panic!("boom");
        }
    }

    fn panicking_library(
        launching: &Launching<'_>,
    ) -> Result<Launched<LibraryCommand>, RuntimeError> {
        RESTART_LIBRARY_CALLS.fetch_add(1, Ordering::SeqCst);
        let thread = crate::driver::spawn_loop::<LibraryCommand, Message, _>(
            crate::registry::row(Driver::Library),
            PanicOnBoot,
            launching.mailbox,
        )?;
        Ok(Launched {
            thread,
            spectrum: None,
        })
    }

    fn panicking_config(
        launching: &Launching<'_>,
    ) -> Result<Launched<ConfigCommand>, RuntimeError> {
        RESTART_CONFIG_CALLS.fetch_add(1, Ordering::SeqCst);
        let thread = crate::driver::spawn_loop::<ConfigCommand, Message, _>(
            crate::registry::row(Driver::Config),
            PanicOnBoot,
            launching.mailbox,
        )?;
        Ok(Launched {
            thread,
            spectrum: None,
        })
    }

    struct RestartRow {
        driver: Driver,
        deaths: usize,
        launches: usize,
        calls: &'static AtomicUsize,
    }

    fn launchers_for(driver: Driver) -> Launchers {
        if driver == Driver::Library {
            Launchers {
                library: panicking_library,
                ..Launchers::idle()
            }
        } else {
            Launchers {
                config: panicking_config,
                ..Launchers::idle()
            }
        }
    }

    #[rstest]
    #[case::config_degrades_without_a_relaunch(RestartRow {
        driver: Driver::Config,
        deaths: 1,
        launches: 1,
        calls: &RESTART_CONFIG_CALLS,
    })]
    #[case::library_restarts_once_then_degrades(RestartRow {
        driver: Driver::Library,
        deaths: 2,
        launches: 2,
        calls: &RESTART_LIBRARY_CALLS,
    })]
    fn restart_follows_the_row(#[case] row: RestartRow) {
        row.calls.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let launchers = launchers_for(row.driver);
        let mut runtime = Runtime::boot(
            Startup::default(),
            &stub_paths(directory.path()),
            &launchers,
        )
        .unwrap();

        for _ in 0..row.deaths {
            let died = runtime.wiring.mailbox.recv_timeout(RECV_TIMEOUT).unwrap();
            runtime.step(died);
        }

        assert_eq!(row.calls.load(Ordering::SeqCst), row.launches);
        assert!(runtime.model.workspace.toast.is_some());
        runtime.drain();
    }
}
