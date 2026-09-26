use std::{collections::HashSet, mem::discriminant};

use crossbeam_channel::Receiver as CmdReceiver;
use kernel::{AudioCmd, AudioEvent, Delivery, Outbox, update::Machine};

use crate::{
    EngineConfig,
    deck::Deck,
    engine::{
        driver::perform,
        effect::EngineMessage,
        state::{Engine, Live},
    },
};

pub(crate) struct Worker {
    pub(crate) state: Engine,
    pub(crate) deck: Deck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Running,
    Closed,
}

pub(crate) fn boot(config: EngineConfig, deck: &mut Deck) -> Engine {
    let opened = deck.open(config.device.clone(), 1.0);
    let mut state = Engine::Live(Live::new(config));
    deliver(&mut state, EngineMessage::Opened(opened), deck);
    state
}

pub(crate) fn audio_thread<O: Outbox<AudioEvent>>(
    commands: &CmdReceiver<AudioCmd>,
    mut worker: Worker,
    outbox: &O,
) {
    if let Flow::Closed = flush(&mut worker.deck, outbox) {
        return;
    }
    let heard = worker.deck.heard().clone();
    loop {
        crossbeam_channel::select! {
            recv(commands) -> received => {
                let Ok(first) = received else { return; };
                let mut batch = vec![first];
                batch.extend(commands.try_iter());
                for cmd in coalesced(batch) {
                    deliver(&mut worker.state, EngineMessage::Cmd(cmd), &mut worker.deck);
                }
            },
            recv(heard) -> event => {
                if let Ok(event) = event {
                    for message in worker.deck.landed(event) {
                        deliver(&mut worker.state, message, &mut worker.deck);
                    }
                }
            },
        }
        if let Flow::Closed = flush(&mut worker.deck, outbox) {
            return;
        }
    }
}

fn flush<O: Outbox<AudioEvent>>(deck: &mut Deck, outbox: &O) -> Flow {
    for fact in deck.drain_facts() {
        if let Delivery::Closed = outbox.send(fact) {
            return Flow::Closed;
        }
    }
    Flow::Running
}

pub(crate) fn coalesced(batch: Vec<AudioCmd>) -> Vec<AudioCmd> {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for cmd in batch.into_iter().rev() {
        match &cmd {
            AudioCmd::Load { .. } | AudioCmd::Stop => {
                seen.clear();
                kept.push(cmd);
            }
            AudioCmd::Volume(_) | AudioCmd::SetSpeed(_) | AudioCmd::Seek(_) => {
                if seen.insert(discriminant(&cmd)) {
                    kept.push(cmd);
                }
            }
            AudioCmd::Pause(_)
            | AudioCmd::Preload { .. }
            | AudioCmd::SetCrossfade(_)
            | AudioCmd::SetReplaygain(_)
            | AudioCmd::SetDevice(_)
            | AudioCmd::ListDevices => kept.push(cmd),
        }
    }
    kept.reverse();
    kept
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

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use kernel::{
        AudioCmd,
        AudioEvent,
        AudioFailure,
        Delivery,
        Outbox,
        Playback,
        domain::{Bounded, Crossfade, Percent, Replaygain, Revision, Speed},
    };
    use rstest::rstest;

    use crate::{
        EngineConfig,
        UnityVolume,
        deck::Deck,
        engine::thread::{Worker, audio_thread, boot, coalesced},
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

    fn live_without_hardware() -> Worker {
        let (spectrum, _spectrum_tap) = tap::new_tap();
        let mut deck = Deck::new(spectrum);
        let state = boot(config(0, Replaygain::Off, UnityVolume::Free), &mut deck);
        Worker { state, deck }
    }

    fn missing_file_load() -> AudioCmd {
        AudioCmd::Load {
            path: "/no/such/sifr-test-file".into(),
            gain: None,
            revision: Revision::default().next(),
        }
    }

    fn load(path: &str) -> AudioCmd {
        AudioCmd::Load {
            path: PathBuf::from(path),
            gain: None,
            revision: Revision::default(),
        }
    }

    struct FakeOutbox {
        sent: Arc<Mutex<Vec<AudioEvent>>>,
        reply: Delivery,
    }

    impl Outbox<AudioEvent> for FakeOutbox {
        fn send(&self, fact: AudioEvent) -> Delivery {
            self.sent.lock().unwrap().push(fact);
            self.reply
        }
    }

    fn wait_for(
        sent: &Arc<Mutex<Vec<AudioEvent>>>,
        timeout: Duration,
    ) -> Vec<AudioEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let guard = sent.lock().unwrap();
                if !guard.is_empty() {
                    return guard.clone();
                }
            }
            if Instant::now() >= deadline {
                return Vec::new();
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_missing_file_load_reports_a_decode_error_without_hardware() {
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let outbox_sent = Arc::clone(&sent);
        let handle = thread::spawn(move || {
            let worker = live_without_hardware();
            let outbox = FakeOutbox {
                sent: outbox_sent,
                reply: Delivery::Sent,
            };
            audio_thread(&command_receiver, worker, &outbox);
        });

        assert!(command_sender.send(missing_file_load()).is_ok());
        let events = wait_for(&sent, Duration::from_secs(2));
        assert!(matches!(
            events.as_slice(),
            [AudioEvent::Error(AudioFailure::Decode { .. })]
        ));

        drop(command_sender);
        assert!(handle.join().is_ok());
    }

    #[test]
    fn an_idle_engine_reports_nothing_until_a_command_arrives() {
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let outbox_sent = Arc::clone(&sent);
        let handle = thread::spawn(move || {
            let worker = live_without_hardware();
            let outbox = FakeOutbox {
                sent: outbox_sent,
                reply: Delivery::Sent,
            };
            audio_thread(&command_receiver, worker, &outbox);
        });

        thread::sleep(Duration::from_millis(250));
        assert!(sent.lock().unwrap().is_empty());

        assert!(command_sender.send(missing_file_load()).is_ok());
        let events = wait_for(&sent, Duration::from_secs(2));
        assert!(matches!(
            events.as_slice(),
            [AudioEvent::Error(AudioFailure::Decode { .. })]
        ));

        drop(command_sender);
        assert!(handle.join().is_ok());
    }

    #[test]
    fn a_closed_outbox_ends_the_loop() {
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let handle = thread::spawn(move || {
            let worker = live_without_hardware();
            let outbox = FakeOutbox {
                sent: Arc::new(Mutex::new(Vec::new())),
                reply: Delivery::Closed,
            };
            audio_thread(&command_receiver, worker, &outbox);
        });

        assert!(command_sender.send(missing_file_load()).is_ok());

        let (finished, joined) = crossbeam_channel::bounded(1);
        thread::spawn(move || {
            let _ = finished.send(handle.join().is_ok());
        });
        assert_eq!(joined.recv_timeout(Duration::from_secs(1)), Ok(true));
        drop(command_sender);
    }

    #[rstest]
    #[case::two_volumes(
        vec![
            AudioCmd::Volume(Percent::clamped(10)),
            AudioCmd::Volume(Percent::clamped(20)),
        ],
        vec![AudioCmd::Volume(Percent::clamped(20))]
    )]
    #[case::volume_speed_volume(
        vec![
            AudioCmd::Volume(Percent::clamped(10)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
            AudioCmd::Volume(Percent::clamped(30)),
        ],
        vec![
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
            AudioCmd::Volume(Percent::clamped(30)),
        ]
    )]
    #[case::seeks_across_a_load(
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            load("/b"),
            AudioCmd::Seek(Duration::from_secs(2)),
            AudioCmd::Seek(Duration::from_secs(3)),
        ],
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            load("/b"),
            AudioCmd::Seek(Duration::from_secs(3)),
        ]
    )]
    #[case::stop_splits(
        vec![
            AudioCmd::Volume(Percent::clamped(1)),
            AudioCmd::Stop,
            AudioCmd::Volume(Percent::clamped(2)),
        ],
        vec![
            AudioCmd::Volume(Percent::clamped(1)),
            AudioCmd::Stop,
            AudioCmd::Volume(Percent::clamped(2)),
        ]
    )]
    #[case::others_untouched(
        vec![
            AudioCmd::Pause(Playback::Playing),
            AudioCmd::Pause(Playback::Playing),
            AudioCmd::ListDevices,
        ],
        vec![
            AudioCmd::Pause(Playback::Playing),
            AudioCmd::Pause(Playback::Playing),
            AudioCmd::ListDevices,
        ]
    )]
    #[case::empty(Vec::new(), Vec::new())]
    fn coalesced_keeps_the_last_idempotent_command(
        #[case] batch: Vec<AudioCmd>,
        #[case] expected: Vec<AudioCmd>,
    ) {
        assert_eq!(coalesced(batch), expected);
    }
}
