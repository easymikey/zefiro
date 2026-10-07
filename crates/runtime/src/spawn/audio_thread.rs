use audio::tap::SpectrumTap;
use kernel::{cmd::AudioCmd, domain::driver::DriverName};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::SpawnError,
    registry,
    spawn_setup::{CALLBACK_SLOTS, SpawnSetup},
};

pub(crate) fn spawn_audio(
    setup: &SpawnSetup<'_>,
) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
    let settings = setup.audio_settings.clone();
    let (spectrum_sender, spectrum_receiver) = crossbeam_channel::bounded(1);
    let (callback_sender, callback_receiver) =
        crossbeam_channel::bounded(CALLBACK_SLOTS);
    let row = registry::row(DriverName::Audio);
    let run_job = audio::deck::job::AudioJob::run;
    let thread = DriverLoop::<audio::AudioDriver, _> {
        row,
        inbox: setup.inbox.clone(),
        callback_receiver,
        message: None,
        run_job,
    }
    .spawn(move || {
        let (driver, spectrum_tap) = audio::AudioDriver::new(settings, callback_sender);
        if let Err(unclaimed) = spectrum_sender.send(spectrum_tap) {
            drop(unclaimed.into_inner());
        }
        driver
    })?;
    let spectrum_tap =
        spectrum_receiver
            .recv()
            .map_err(|_disconnected| SpawnError::TapLost {
                driver_name: row.driver_name,
            })?;
    Ok((thread, spectrum_tap))
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use audio::tap::SpectrumTap;
    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        cmd::AudioCmd,
        domain::{
            direction::Direction,
            driver::{DriverName, DriverStatus},
            setting_row::SettingRow,
            startup::Startup,
        },
        message::Message,
    };

    use crate::{
        driver_thread::{Congestion, DriverThread},
        error::SpawnError,
        runtime::Runtime,
        spawn::{
            SpawnSetup,
            Spawners,
            tests::{RECV_TIMEOUT, boom, idle_spawners, spawn_audio_loop, stub_paths},
        },
    };

    fn died_from_replay_gain_step(runtime: &mut Runtime) -> Message {
        let stepped = runtime.deliver(Message::Step {
            row: SettingRow::ReplayGain,
            direction: Direction::Next,
        });
        assert_eq!(stepped, Ok(()));
        runtime
            .wiring
            .inbox_receiver
            .recv_timeout(RECV_TIMEOUT)
            .unwrap()
    }

    static AUDIO_RESTART_SPAWNS: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static AUDIO_RESTART_CMD_SENDER: RefCell<Option<Sender<AudioCmd>>> =
            const { RefCell::new(None) };
    }

    fn panic_once_then_record_audio(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        let cmd_sender = AUDIO_RESTART_CMD_SENDER
            .with(|cmd_sender_cell| cmd_sender_cell.borrow().clone())
            .unwrap();
        spawn_audio_loop(
            move |cmd_receiver: &Receiver<AudioCmd>,
                  _: &Sender<Message>,
                  _: &Congestion| {
                if AUDIO_RESTART_SPAWNS.fetch_add(1, Ordering::SeqCst) == 0 {
                    cmd_receiver
                        .recv()
                        .map_or_else(|_closed| boom(), |_cmd| boom());
                }
                while let Ok(cmd) = cmd_receiver.recv() {
                    if cmd_sender.send(cmd).is_err() {
                        return Ok(());
                    }
                }
                Ok(())
            },
            setup,
        )
    }

    #[test]
    fn a_panicking_audio_driver_is_restarted_and_started() {
        AUDIO_RESTART_SPAWNS.store(0, Ordering::SeqCst);
        let (cmd_sender, cmd_receiver) = unbounded();
        AUDIO_RESTART_CMD_SENDER
            .with(|cmd_sender_cell| *cmd_sender_cell.borrow_mut() = Some(cmd_sender));
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: panic_once_then_record_audio,
            ..idle_spawners()
        };
        let mut runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();

        let died = died_from_replay_gain_step(&mut runtime);
        runtime.deliver(died).unwrap();

        let audio_cmds: Vec<AudioCmd> = (0..4)
            .map(|_| cmd_receiver.recv_timeout(RECV_TIMEOUT).unwrap())
            .collect();
        assert!(matches!(audio_cmds[0], AudioCmd::ListDevices));
        assert!(matches!(audio_cmds[1], AudioCmd::SetDevice(_)));
        assert!(matches!(audio_cmds[2], AudioCmd::SetCrossfade(_)));
        assert!(matches!(audio_cmds[3], AudioCmd::SetReplayGain(_)));
        assert_eq!(
            *runtime.model.drivers.status(DriverName::Audio),
            DriverStatus::Running
        );
        runtime.drain();
    }

    static DROP_SPAWN_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    static FIRST_DROP_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
    static SECOND_SPAWN_ORDER: AtomicUsize = AtomicUsize::new(usize::MAX);
    static SEQUENCED_SPAWNS: AtomicUsize = AtomicUsize::new(0);

    struct SequencedAudio;

    impl Drop for SequencedAudio {
        fn drop(&mut self) {
            let order = DROP_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            FIRST_DROP_ORDER.store(order, Ordering::SeqCst);
        }
    }

    fn sequenced_audio(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), SpawnError> {
        if SEQUENCED_SPAWNS.fetch_add(1, Ordering::SeqCst) == 1 {
            let order = DROP_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            SECOND_SPAWN_ORDER.store(order, Ordering::SeqCst);
        }
        spawn_audio_loop(
            |cmd_receiver: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| {
                let _sequenced = SequencedAudio;
                cmd_receiver
                    .recv()
                    .map_or_else(|_closed| boom(), |_cmd| boom())
            },
            setup,
        )
    }

    #[test]
    fn the_dead_thread_is_joined_before_the_new_one_spawns() {
        DROP_SPAWN_SEQUENCE.store(0, Ordering::SeqCst);
        FIRST_DROP_ORDER.store(usize::MAX, Ordering::SeqCst);
        SECOND_SPAWN_ORDER.store(usize::MAX, Ordering::SeqCst);
        SEQUENCED_SPAWNS.store(0, Ordering::SeqCst);
        let directory = tempfile::tempdir().unwrap();
        let spawners = Spawners {
            audio: sequenced_audio,
            ..idle_spawners()
        };
        let mut runtime = Runtime::start(
            Startup::default(),
            &stub_paths(directory.path()),
            &spawners,
        )
        .unwrap();

        let died = died_from_replay_gain_step(&mut runtime);
        runtime.deliver(died).unwrap();

        assert!(
            FIRST_DROP_ORDER.load(Ordering::SeqCst)
                < SECOND_SPAWN_ORDER.load(Ordering::SeqCst)
        );
        runtime.drain();
    }
}
