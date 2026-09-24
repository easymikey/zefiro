use crossbeam_channel::{Receiver as CmdReceiver, RecvTimeoutError};
use kernel::{AudioCmd, AudioEvent, AudioFailure, update::Machine};

use crate::{
    EngineConfig,
    deck::{Deck, Landed},
    engine::{
        driver::perform,
        effect::{EngineMessage, Preload},
        phase::Phase,
        state::{Engine, Live, Muted},
    },
};

pub(crate) struct Worker {
    pub(crate) deck: Deck,
}

pub(crate) fn boot(config: EngineConfig, deck: &mut Deck) -> Engine {
    let opened = deck.open(config.device.clone(), 1.0);
    let mut state = Engine::Live(Live::new(config));
    deliver(&mut state, EngineMessage::Opened(opened), deck);
    state
}

pub(crate) fn audio_thread(
    command_receiver: &CmdReceiver<AudioCmd>,
    mut state: Engine,
    mut worker: Worker,
) {
    loop {
        match wait(is_idle(&state), command_receiver) {
            Ok(cmd) => deliver(&mut state, EngineMessage::Cmd(cmd), &mut worker.deck),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for cmd in command_receiver.try_iter() {
            deliver(&mut state, EngineMessage::Cmd(cmd), &mut worker.deck);
        }
        if matches!(is_idle(&state), Activity::Busy) {
            tick(&mut state, &mut worker.deck);
        }
    }
}

#[derive(Clone, Copy)]
enum Activity {
    Idle,
    Busy,
}

fn is_idle(state: &Engine) -> Activity {
    match state {
        Engine::Muted(Muted { pending: None, .. })
        | Engine::Live(Live {
            phase: Phase::Idle, ..
        }) => Activity::Idle,
        Engine::Muted(_) | Engine::Live(_) => Activity::Busy,
    }
}

fn wait(
    activity: Activity,
    command_receiver: &CmdReceiver<AudioCmd>,
) -> Result<AudioCmd, RecvTimeoutError> {
    match activity {
        Activity::Idle => command_receiver
            .recv()
            .map_err(|_| RecvTimeoutError::Disconnected),
        Activity::Busy => command_receiver.recv_timeout(Engine::TICK),
    }
}

fn deliver(state: &mut Engine, message: EngineMessage, deck: &mut Deck) {
    let mut next = Some(message);
    while let Some(current) = next {
        next = match state.update(current) {
            Ok(effect) => perform(effect, deck),
            Err(rejection) => {
                deck.send(AudioEvent::Rejected(rejection));
                None
            }
        };
    }
}

fn tick(state: &mut Engine, deck: &mut Deck) {
    if let Some(fault) = deck.poll_fault() {
        deliver(state, EngineMessage::Failed(fault), deck);
    }
    if let Some(outcome) = deck.poll_decode() {
        deliver(state, EngineMessage::Decoded(outcome), deck);
    }
    if let Some(landed) = deck.poll_preload() {
        deliver(state, preloaded(landed), deck);
    }
    if let Some((queue_len, position)) = deck.observe() {
        deliver(
            state,
            EngineMessage::Observed {
                queue_len,
                position,
            },
            deck,
        );
    }
}

fn preloaded(landed: Result<Landed, AudioFailure>) -> EngineMessage {
    EngineMessage::Preloaded(landed.map(Preload::from))
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::Sender as EventSender;
    use kernel::{
        AudioCmd,
        AudioFailure,
        Bounded,
        Message,
        domain::{Crossfade, Replaygain, Revision},
    };

    use crate::{
        EngineConfig,
        UnityVolume,
        deck::Deck,
        engine::{
            state::{Engine, Live},
            thread::{Worker, audio_thread},
        },
        tap,
    };

    fn config(
        crossfade_secs: u64,
        replaygain: Replaygain,
        unity_volume: UnityVolume,
    ) -> EngineConfig {
        EngineConfig {
            crossfade: Crossfade::clamped(Duration::from_secs(crossfade_secs)),
            replaygain,
            unity_volume,
            device: None,
        }
    }

    fn live_without_hardware(events: EventSender<Message>) -> (Engine, Worker) {
        let (ring, _spectrum) = tap::new_tap();
        let deck = Deck::new(events, ring);
        let engine =
            Engine::Live(Live::new(config(0, Replaygain::Off, UnityVolume::Free)));
        (engine, Worker { deck })
    }

    fn missing_file_load() -> AudioCmd {
        AudioCmd::Load {
            path: "/no/such/sifr-test-file".into(),
            gain: None,
            revision: Revision::default().next(),
        }
    }

    #[test]
    fn a_missing_file_load_reports_a_decode_error_without_hardware() {
        let (event_sender, event_receiver) = crossbeam_channel::unbounded::<Message>();
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let handle = thread::spawn(move || {
            let (engine, worker) = live_without_hardware(event_sender);
            audio_thread(&command_receiver, engine, worker);
        });

        assert!(command_sender.send(missing_file_load()).is_ok());
        assert!(matches!(
            event_receiver.recv_timeout(Duration::from_secs(2)),
            Ok(Message::Audio(kernel::AudioEvent::Error(
                AudioFailure::Decode { .. }
            )))
        ));

        drop(command_sender);
        assert!(handle.join().is_ok());
    }

    #[test]
    fn an_idle_engine_reports_nothing_until_a_command_arrives() {
        let (event_sender, event_receiver) = crossbeam_channel::unbounded::<Message>();
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let handle = thread::spawn(move || {
            let (engine, worker) = live_without_hardware(event_sender);
            audio_thread(&command_receiver, engine, worker);
        });

        thread::sleep(Duration::from_millis(250));
        assert!(matches!(
            event_receiver.try_recv(),
            Err(crossbeam_channel::TryRecvError::Empty)
        ));

        assert!(command_sender.send(missing_file_load()).is_ok());
        assert!(matches!(
            event_receiver.recv_timeout(Duration::from_secs(2)),
            Ok(Message::Audio(kernel::AudioEvent::Error(
                AudioFailure::Decode { .. }
            )))
        ));

        drop(command_sender);
        assert!(handle.join().is_ok());
    }
}
