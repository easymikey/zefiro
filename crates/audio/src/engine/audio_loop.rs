use std::{collections::HashSet, mem::discriminant};

use crossbeam_channel::Receiver as CmdReceiver;
use kernel::{
    AudioCmd,
    AudioEvent,
    Outbox,
    Refusals,
    SendError,
    domain::Speed,
    update::Machine,
};

use crate::{
    AudioLoop,
    EngineConfig,
    deck::Deck,
    engine::{
        effect::EngineMessage,
        perform::perform,
        state::{Engine, Live},
    },
};

fn start(config: EngineConfig, deck: &mut Deck) -> Engine {
    let speed = Speed::default();
    let opened = deck.open(config.device.clone(), speed.get());
    let mut state = Engine::Live(Live::new(config, speed));
    step(&mut state, EngineMessage::Opened(opened), deck);
    state
}

pub(crate) fn run_audio_loop<O: Outbox<AudioEvent> + Refusals>(
    commands: &CmdReceiver<AudioCmd>,
    setup: AudioLoop,
    outbox: &O,
) {
    let AudioLoop { config, spectrum } = setup;
    let mut deck = Deck::new(spectrum);
    let mut engine = start(config, &mut deck);
    if flush(&mut deck, outbox).is_err() {
        return;
    }
    let heard = deck.events().clone();
    loop {
        crossbeam_channel::select! {
            recv(commands) -> received => {
                let Ok(first) = received else { return; };
                let mut batch = vec![first];
                batch.extend(commands.try_iter());
                for cmd in keep_last_idempotent(batch) {
                    step(&mut engine, EngineMessage::Cmd(cmd), &mut deck);
                }
            },
            recv(heard) -> event => {
                if let Ok(event) = event {
                    for message in deck.messages_for(event) {
                        step(&mut engine, message, &mut deck);
                    }
                }
            },
        }
        if flush(&mut deck, outbox).is_err() {
            return;
        }
    }
}

fn ignore_full(sent: Result<(), SendError>) -> Result<(), SendError> {
    match sent {
        Err(SendError::Closed) => Err(SendError::Closed),
        Ok(()) | Err(SendError::Full) => Ok(()),
    }
}

fn flush<O: Outbox<AudioEvent> + Refusals>(
    deck: &mut Deck,
    outbox: &O,
) -> Result<(), SendError> {
    for event in deck.drain_events() {
        ignore_full(outbox.send(event))?;
    }
    for input in deck.drain_refused() {
        ignore_full(outbox.refused(input))?;
    }
    Ok(())
}

pub(crate) fn keep_last_idempotent(batch: Vec<AudioCmd>) -> Vec<AudioCmd> {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for cmd in batch.into_iter().rev() {
        match &cmd {
            AudioCmd::Load(_) | AudioCmd::Stop => {
                seen.clear();
                kept.push(cmd);
            }
            AudioCmd::SetSpeed(_) | AudioCmd::Seek(_) => {
                if seen.insert(discriminant(&cmd)) {
                    kept.push(cmd);
                }
            }
            AudioCmd::Playback(_)
            | AudioCmd::Preload(_)
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
        EngineMessage::Finished(_) => "finished",
        EngineMessage::Cued => "cued",
        EngineMessage::Ramped(_) => "ramped",
        EngineMessage::DevicesListed(_) => "devices_listed",
    }
}

fn step(state: &mut Engine, message: EngineMessage, deck: &mut Deck) {
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

    use crossbeam_channel::Sender;
    use kernel::{
        AudioCmd,
        AudioError,
        AudioEvent,
        Outbox,
        Playback,
        Refusals,
        SendError,
        TrackRequest,
        domain::{Bounded, Crossfade, OutputDevice, Replaygain, Revision, Speed},
    };
    use rstest::rstest;

    use crate::{
        AudioLoop,
        EngineConfig,
        engine::audio_loop::{keep_last_idempotent, run_audio_loop},
    };

    type Sent = Arc<Mutex<Vec<AudioEvent>>>;

    fn spawn_loop(
        reply: Result<(), SendError>,
    ) -> (Sender<AudioCmd>, Sent, thread::JoinHandle<()>) {
        let (command_sender, command_receiver) =
            crossbeam_channel::unbounded::<AudioCmd>();
        let sent: Sent = Arc::new(Mutex::new(Vec::new()));
        let outbox_sent = Arc::clone(&sent);
        let handle = thread::spawn(move || {
            let (thread, _spectrum_tap) = AudioLoop::new(EngineConfig {
                crossfade: Crossfade::clamped(Duration::ZERO),
                replaygain: Replaygain::Off,
                device: OutputDevice::SystemDefault,
            });
            let outbox = FakeOutbox {
                sent: outbox_sent,
                reply,
            };
            run_audio_loop(&command_receiver, thread, &outbox);
        });
        (command_sender, sent, handle)
    }

    fn missing_file_load() -> AudioCmd {
        AudioCmd::Load(TrackRequest {
            path: "/no/such/sifr-test-file".into(),
            gain: None,
            revision: Revision::default().next(),
        })
    }

    fn load(path: &str) -> AudioCmd {
        AudioCmd::Load(TrackRequest {
            path: PathBuf::from(path),
            gain: None,
            revision: Revision::default(),
        })
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
        let (command_sender, sent, handle) = spawn_loop(Ok(()));

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
        let (command_sender, sent, handle) = spawn_loop(Ok(()));

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
        let (command_sender, _sent, handle) = spawn_loop(Err(SendError::Closed));

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
        assert_eq!(keep_last_idempotent(batch), expected);
    }
}
