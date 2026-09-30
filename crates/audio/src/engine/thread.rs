use std::{collections::HashSet, mem::discriminant};

use crossbeam_channel::Receiver as CmdReceiver;
use kernel::{AudioCmd, AudioEvent, Outbox, Refusals, SendError, update::Machine};

use crate::{
    EngineConfig,
    deck::Deck,
    engine::{
        effect::EngineMessage,
        perform::perform,
        state::{Engine, Live},
    },
};

pub(crate) struct EngineThread {
    pub(crate) engine: Engine,
    pub(crate) deck: Deck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Running,
    Closed,
}

pub(crate) fn start(config: EngineConfig, deck: &mut Deck) -> Engine {
    let opened = deck.open(config.device.clone(), 1.0);
    let mut state = Engine::Live(Live::new(config));
    deliver(&mut state, EngineMessage::Opened(opened), deck);
    state
}

pub(crate) fn audio_thread<O: Outbox<AudioEvent> + Refusals>(
    commands: &CmdReceiver<AudioCmd>,
    mut thread: EngineThread,
    outbox: &O,
) {
    if let Flow::Closed = flush(&mut thread.deck, outbox) {
        return;
    }
    let heard = thread.deck.heard().clone();
    loop {
        crossbeam_channel::select! {
            recv(commands) -> received => {
                let Ok(first) = received else { return; };
                let mut batch = vec![first];
                batch.extend(commands.try_iter());
                for cmd in coalesced(batch) {
                    deliver(&mut thread.engine, EngineMessage::Cmd(cmd), &mut thread.deck);
                }
            },
            recv(heard) -> event => {
                if let Ok(event) = event {
                    for message in thread.deck.messages_for(event) {
                        deliver(&mut thread.engine, message, &mut thread.deck);
                    }
                }
            },
        }
        if let Flow::Closed = flush(&mut thread.deck, outbox) {
            return;
        }
    }
}

fn flush<O: Outbox<AudioEvent> + Refusals>(deck: &mut Deck, outbox: &O) -> Flow {
    for event in deck.drain_events() {
        if let Err(SendError::Closed) = outbox.send(event) {
            return Flow::Closed;
        }
    }
    for input in deck.drain_refused() {
        if let Err(SendError::Closed) = outbox.refused(input) {
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
            AudioCmd::SetSpeed(_) | AudioCmd::Seek(_) => {
                if seen.insert(discriminant(&cmd)) {
                    kept.push(cmd);
                }
            }
            AudioCmd::Playback(_)
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

fn input_name(message: &EngineMessage) -> &'static str {
    match message {
        EngineMessage::Cmd(cmd) => cmd.into(),
        EngineMessage::Opened(_) => "opened",
        EngineMessage::Decoded(_) => "decoded",
        EngineMessage::Preloaded(_) => "preloaded",
        EngineMessage::Failed(_) => "failed",
        EngineMessage::Retiring { .. } => "retiring",
        EngineMessage::Finished(_) => "finished",
        EngineMessage::Cued => "cued",
        EngineMessage::Ramped(_) => "ramped",
        EngineMessage::DevicesListed(_) => "devices_listed",
    }
}

fn deliver(state: &mut Engine, message: EngineMessage, deck: &mut Deck) {
    let mut next = Some(message);
    while let Some(current) = next {
        let input = input_name(&current);
        next = if let Ok(effect) = state.update(current) {
            perform(effect, deck)
        } else {
            deck.refuse(input);
            None
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
        AudioError,
        AudioEvent,
        Outbox,
        Playback,
        Refusals,
        SendError,
        domain::{Bounded, Crossfade, OutputDevice, Replaygain, Revision, Speed},
    };
    use rstest::rstest;

    use crate::{
        EngineConfig,
        UnityVolume,
        deck::Deck,
        engine::thread::{EngineThread, audio_thread, coalesced, start},
        tap,
    };

    fn config(
        crossfade_seconds: u64,
        replaygain: Replaygain,
        unity_volume: UnityVolume,
    ) -> EngineConfig {
        EngineConfig {
            crossfade: Crossfade::clamped(Duration::from_secs(crossfade_seconds)),
            replaygain,
            unity_volume,
            device: OutputDevice::SystemDefault,
        }
    }

    fn live_without_hardware() -> EngineThread {
        let (spectrum, _spectrum_tap) = tap::new_tap();
        let mut deck = Deck::new(spectrum);
        let state = start(config(0, Replaygain::Off, UnityVolume::Free), &mut deck);
        EngineThread {
            engine: state,
            deck,
        }
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
        reply: Result<(), SendError>,
    }

    impl Refusals for FakeOutbox {
        fn refused(&self, _input: &'static str) -> Result<(), SendError> {
            self.reply
        }
    }

    impl Outbox<AudioEvent> for FakeOutbox {
        fn send(&self, event: AudioEvent) -> Result<(), SendError> {
            self.sent.lock().unwrap().push(event);
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
            let thread = live_without_hardware();
            let outbox = FakeOutbox {
                sent: outbox_sent,
                reply: Ok(()),
            };
            audio_thread(&command_receiver, thread, &outbox);
        });

        assert!(command_sender.send(missing_file_load()).is_ok());
        let events = wait_for(&sent, Duration::from_secs(2));
        assert!(matches!(
            events.as_slice(),
            [AudioEvent::Error(AudioError::Decode { .. })]
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
            let thread = live_without_hardware();
            let outbox = FakeOutbox {
                sent: outbox_sent,
                reply: Ok(()),
            };
            audio_thread(&command_receiver, thread, &outbox);
        });

        thread::sleep(Duration::from_millis(250));
        assert!(sent.lock().unwrap().is_empty());

        assert!(command_sender.send(missing_file_load()).is_ok());
        let events = wait_for(&sent, Duration::from_secs(2));
        assert!(matches!(
            events.as_slice(),
            [AudioEvent::Error(AudioError::Decode { .. })]
        ));

        drop(command_sender);
        assert!(handle.join().is_ok());
    }

    #[test]
    fn a_closed_outbox_ends_the_loop() {
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let handle = thread::spawn(move || {
            let thread = live_without_hardware();
            let outbox = FakeOutbox {
                sent: Arc::new(Mutex::new(Vec::new())),
                reply: Err(SendError::Closed),
            };
            audio_thread(&command_receiver, thread, &outbox);
        });

        assert!(command_sender.send(missing_file_load()).is_ok());

        let (finished, joined) = crossbeam_channel::bounded(1);
        thread::spawn(move || {
            finished.send(handle.join().is_ok()).unwrap();
        });
        assert_eq!(joined.recv_timeout(Duration::from_secs(1)), Ok(true));
        drop(command_sender);
    }

    #[rstest]
    #[case::two_speeds(
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ],
        vec![AudioCmd::SetSpeed(Speed::clamped(2.0))]
    )]
    #[case::speed_seek_speed(
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::Seek(Duration::from_secs(1)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ],
        vec![
            AudioCmd::Seek(Duration::from_secs(1)),
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
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
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::Stop,
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ],
        vec![
            AudioCmd::SetSpeed(Speed::clamped(1.5)),
            AudioCmd::Stop,
            AudioCmd::SetSpeed(Speed::clamped(2.0)),
        ]
    )]
    #[case::others_untouched(
        vec![
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::ListDevices,
        ],
        vec![
            AudioCmd::Playback(Playback::Playing),
            AudioCmd::Playback(Playback::Playing),
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
