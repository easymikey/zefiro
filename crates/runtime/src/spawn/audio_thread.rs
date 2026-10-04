use audio::tap::SpectrumTap;
use kernel::{cmd::AudioCmd, domain::driver::DriverName, update::machine::Machine};

#[cfg(test)] use crate::driver_thread::spawn_idle;
use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::Error,
    jobs::{Jobs, LoopEffect},
    registry,
    spawn_setup::SpawnSetup,
};

#[cfg(test)]
pub(crate) fn idle_audio(
    setup: &SpawnSetup<'_>,
) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
    let thread = spawn_idle(registry::row(DriverName::Audio), setup.inbox)?;
    Ok((thread, SpectrumTap::silent()))
}

pub(crate) fn audio_split(
    effect: audio::engine::effect::EngineEffect,
) -> LoopEffect<
    audio::engine::effect::EngineEffect,
    audio::deck::job::AudioJob,
    <audio::AudioDriver as Machine>::Message,
> {
    match effect {
        audio::engine::effect::EngineEffect::Run(job) => LoopEffect::Run(job),
        effect @ (audio::engine::effect::EngineEffect::Silence
        | audio::engine::effect::EngineEffect::Open { .. }
        | audio::engine::effect::EngineEffect::StartLoad { .. }
        | audio::engine::effect::EngineEffect::StartHandover { .. }
        | audio::engine::effect::EngineEffect::Decode(_)
        | audio::engine::effect::EngineEffect::Start(_)
        | audio::engine::effect::EngineEffect::Resume { .. }
        | audio::engine::effect::EngineEffect::Play
        | audio::engine::effect::EngineEffect::Pause
        | audio::engine::effect::EngineEffect::Seek(_)
        | audio::engine::effect::EngineEffect::SetGain(_)
        | audio::engine::effect::EngineEffect::Arm(_)
        | audio::engine::effect::EngineEffect::Crossfade { .. }
        | audio::engine::effect::EngineEffect::CancelCrossfade
        | audio::engine::effect::EngineEffect::Ramp { .. }
        | audio::engine::effect::EngineEffect::DropOutgoing
        | audio::engine::effect::EngineEffect::SetSpeed(_)
        | audio::engine::effect::EngineEffect::Clear(_)
        | audio::engine::effect::EngineEffect::Preload { .. }
        | audio::engine::effect::EngineEffect::RestartGapless(_)
        | audio::engine::effect::EngineEffect::Promote(_)
        | audio::engine::effect::EngineEffect::Report
        | audio::engine::effect::EngineEffect::Advance(_)
        | audio::engine::effect::EngineEffect::Stage(_)
        | audio::engine::effect::EngineEffect::Attach(_)
        | audio::engine::effect::EngineEffect::TakeSignals(_)) => {
            LoopEffect::Execute(effect)
        }
    }
}

pub(crate) fn spawn_audio(
    setup: &SpawnSetup<'_>,
) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
    let settings = setup.audio.clone();
    let (tap_sender, tap_receiver) = crossbeam_channel::bounded(1);
    let (deck_sender, heard) = crossbeam_channel::bounded(64);
    let row = registry::row(DriverName::Audio);
    let jobs = Jobs {
        split: audio_split,
        run: audio::deck::job::AudioJob::run,
    };
    let thread = DriverLoop::<audio::AudioDriver, _> {
        row,
        inbox: setup.inbox.clone(),
        heard,
        seed: None,
        jobs,
    }
    .spawn(move || {
        let (driver, spectrum) = audio::AudioDriver::new(settings, deck_sender);
        if let Err(unclaimed) = tap_sender.send(spectrum) {
            drop(unclaimed.into_inner());
        }
        driver
    })?;
    let spectrum = tap_receiver
        .recv()
        .map_err(|_disconnected| Error::TapLost { driver: row.driver })?;
    Ok((thread, spectrum))
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
        error::Error,
        runtime::Runtime,
        spawn::{
            Spawners,
            tests::{RECV_TIMEOUT, boom, spawn_audio_loop, stub_paths},
        },
        spawn_setup::SpawnSetup,
    };

    fn died_from_replay_gain_step(runtime: &mut Runtime) -> Message {
        let stepped = runtime.deliver(Message::Step {
            row: SettingRow::ReplayGain,
            direction: Direction::Next,
        });
        assert_eq!(stepped, Ok(()));
        runtime.wiring.mailbox.recv_timeout(RECV_TIMEOUT).unwrap()
    }

    static AUDIO_RESTART_SPAWNS: AtomicUsize = AtomicUsize::new(0);

    thread_local! {
        static AUDIO_RESTART_FORWARD: RefCell<Option<Sender<AudioCmd>>> =
            const { RefCell::new(None) };
    }

    fn panic_once_then_record_audio(
        setup: &SpawnSetup<'_>,
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        let forward = AUDIO_RESTART_FORWARD
            .with(|slot| slot.borrow().clone())
            .unwrap();
        spawn_audio_loop(
            move |inbox: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| {
                if AUDIO_RESTART_SPAWNS.fetch_add(1, Ordering::SeqCst) == 0 {
                    inbox
                        .recv()
                        .map_or_else(|_closed| boom(), |_command| boom());
                }
                while let Ok(command) = inbox.recv() {
                    if forward.send(command).is_err() {
                        return;
                    }
                }
            },
            setup,
        )
    }

    #[test]
    fn a_panicking_audio_driver_is_restarted_and_started() {
        AUDIO_RESTART_SPAWNS.store(0, Ordering::SeqCst);
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
        runtime.deliver(died).unwrap();

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
    ) -> Result<(DriverThread<AudioCmd>, SpectrumTap), Error> {
        if SEQUENCED_SPAWNS.fetch_add(1, Ordering::SeqCst) == 1 {
            let order = DROP_SPAWN_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            SECOND_SPAWN_ORDER.store(order, Ordering::SeqCst);
        }
        spawn_audio_loop(
            |inbox: &Receiver<AudioCmd>, _: &Sender<Message>, _: &Congestion| {
                let _sequenced = SequencedAudio;
                inbox
                    .recv()
                    .map_or_else(|_closed| boom(), |_command| boom());
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
            ..Spawners::idle()
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
