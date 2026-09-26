pub(crate) mod envelope;
mod event;
pub(crate) mod output;
pub(crate) mod source;
mod worker;

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
pub(crate) use event::{DeckEvent, Ticket};
use kernel::{AudioEvent, AudioFailure, Playback, domain::DeviceName};
pub(crate) use source::Landed;

use crate::{
    deck::{
        envelope::{Curve, EnvelopeControl, Envelopes, Flags, Ramp},
        output::{Output, open_output_stream},
        source::{DeckSource, PreloadRequest},
    },
    engine::effect::{EngineMessage, Preload, Slot},
    error::{device_fault, output_lost},
    tap::Handoff,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reopening {
    pub(crate) device: Option<DeviceName>,
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
    pub(crate) opened: DeviceOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceOpen {
    FellBack,
    AsRequested,
}

pub(crate) struct Deck {
    pub(crate) output: Option<Output>,
    source: DeckSource,
    facts: Vec<AudioEvent>,
    wake: Sender<DeckEvent>,
    heard: Receiver<DeckEvent>,
    spectrum: Handoff,
}

impl Deck {
    pub(crate) fn new(spectrum: Handoff) -> Self {
        let (wake, heard) = crossbeam_channel::bounded(64);
        let source = DeckSource::new(wake.clone());
        Self {
            output: None,
            source,
            facts: Vec::new(),
            wake,
            heard,
            spectrum,
        }
    }

    pub(crate) fn send(&mut self, event: AudioEvent) {
        self.facts.push(event);
    }

    pub(crate) fn drain_facts(&mut self) -> std::vec::Drain<'_, AudioEvent> {
        self.facts.drain(..)
    }

    pub(crate) fn heard(&self) -> &Receiver<DeckEvent> {
        &self.heard
    }

    pub(crate) fn landed(&mut self, event: DeckEvent) -> Vec<EngineMessage> {
        match event {
            DeckEvent::Fault(error) => {
                vec![EngineMessage::Failed(output_lost(&error))]
            }
            DeckEvent::Decoded { ticket, outcome } => self
                .source
                .accept_decode((ticket, outcome))
                .map(EngineMessage::Decoded)
                .into_iter()
                .collect(),
            DeckEvent::Preloaded { ticket, outcome } => {
                let Some(landed) = self
                    .source
                    .accept_preload((ticket, outcome), self.output.as_mut())
                else {
                    return Vec::new();
                };
                vec![EngineMessage::Preloaded(landed.map(Preload::from))]
            }
            DeckEvent::DevicesListed(result) => {
                vec![EngineMessage::DevicesListed(result.map_err(device_fault))]
            }
            DeckEvent::Track(ticket) => self.track_events(ticket),
        }
    }

    fn track_events(&self, ticket: Ticket) -> Vec<EngineMessage> {
        let Some(control) = self.source.envelopes.holding(ticket) else {
            return Vec::new();
        };
        let flags = control.take_flags();
        let slot = slot_for(&self.source.envelopes, ticket);
        let mut messages = Vec::new();
        if flags.contains(Flags::CUED) && matches!(slot, Some(Slot::Primary)) {
            messages.push(EngineMessage::Cued);
        }
        if let Some(slot) = slot {
            if flags.contains(Flags::RAMPED) {
                messages.push(EngineMessage::Ramped(slot));
            }
            if flags.contains(Flags::FINISHED) {
                messages.push(EngineMessage::Finished(slot));
            }
        }
        messages
    }

    pub(crate) fn advance(&mut self) {
        self.source.envelopes.primary = self.source.envelopes.queued.take();
    }

    pub(crate) fn playhead(&self) -> Option<Duration> {
        if let Some(control) = self.source.envelopes.primary.as_ref() {
            return Some(control.position());
        }
        self.output.as_ref().map(|output| output.sink.get_pos())
    }

    pub(crate) fn silence(&mut self) {
        self.drop_preload();
        self.output = None;
        self.source.clear_staged();
    }

    pub(crate) fn open(
        &mut self,
        device: Option<DeviceName>,
        speed: f32,
    ) -> Result<Reopening, AudioFailure> {
        let opened = open_output_stream(device, &self.wake).map_err(device_fault)?;
        let (position, playback) = self
            .output
            .as_ref()
            .map_or((Duration::ZERO, Playback::Playing), Output::at);
        drop(self.output.take());
        let mut output = Output::with_stream(opened.stream, speed);
        output.listen(&self.spectrum);
        self.output = Some(output);
        Ok(Reopening {
            device: opened.device,
            position,
            playback,
            opened: opened.opened,
        })
    }

    pub(crate) fn spawn_decode(
        &mut self,
        path: std::path::PathBuf,
    ) -> Option<EngineMessage> {
        self.source
            .spawn_decode(path)
            .map(|fault| EngineMessage::Decoded(Err(fault)))
    }

    pub(crate) fn start_preload(
        &mut self,
        request: PreloadRequest,
    ) -> Option<EngineMessage> {
        self.source
            .start_preload(request)
            .map(|fault| EngineMessage::Preloaded(Err(fault)))
    }

    pub(crate) fn drop_preload(&mut self) {
        self.source.drop_preload();
        if let Some(output) = self.output.as_mut() {
            output.preload = None;
        }
    }

    pub(crate) fn clear_staged(&mut self) {
        self.source.clear_staged();
    }

    pub(crate) fn append_staged(&mut self) {
        self.source.append_staged(self.output.as_mut());
    }

    pub(crate) fn promote(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.promote();
        }
        self.source.envelopes.primary = self.source.envelopes.incoming.take();
    }

    pub(crate) fn retire_primary(&mut self, speed: f32) {
        if let Some(output) = self.output.as_mut() {
            output.retire_sink(speed);
        }
        self.source.envelopes.outgoing = self.source.envelopes.primary.take();
    }

    pub(crate) fn swap_primary(&mut self, speed: f32) {
        if let Some(output) = self.output.as_mut() {
            output.outgoing = None;
            output.swap_sink(speed);
        }
        self.source.envelopes.primary = None;
        self.source.envelopes.outgoing = None;
    }

    pub(crate) fn cue_primary(&mut self, cue: Option<Duration>) {
        if let Some(control) = self.source.envelopes.primary.as_mut() {
            control.cue(cue);
        }
    }

    pub(crate) fn crossfade(&mut self, length: Duration, incoming: f32) {
        if let Some(control) = self.source.envelopes.primary.as_mut() {
            let frames = control.frames(length);
            control.ramp(Ramp {
                from: 1.0,
                to: 0.0,
                curve: Curve::EqualPowerOut,
                frames,
            });
        }
        if let Some(control) = self.source.envelopes.incoming.as_mut() {
            let frames = control.frames(length);
            control.ramp(Ramp {
                from: 0.0,
                to: 1.0,
                curve: Curve::EqualPowerIn,
                frames,
            });
        }
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let Some(preload) = output.preload.as_ref() else {
            return;
        };
        preload.set_volume(incoming);
        preload.play();
    }

    pub(crate) fn unfade(&mut self) {
        if let Some(control) = self.source.envelopes.primary.as_mut() {
            control.ramp(Ramp {
                from: 1.0,
                to: 1.0,
                curve: Curve::EqualPowerIn,
                frames: 0,
            });
        }
        if let Some(control) = self.source.envelopes.incoming.as_mut() {
            control.ramp(Ramp {
                from: 0.0,
                to: 0.0,
                curve: Curve::EqualPowerOut,
                frames: 0,
            });
        }
        if let Some(preload) = self
            .output
            .as_ref()
            .and_then(|output| output.preload.as_ref())
        {
            preload.pause();
        }
    }

    pub(crate) fn drop_outgoing(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.outgoing = None;
        }
        self.source.envelopes.outgoing = None;
    }

    pub(crate) fn retiring_gain(&self) -> f32 {
        let envelope = self
            .source
            .envelopes
            .outgoing
            .as_ref()
            .map_or(0.0, EnvelopeControl::gain);
        let sink = self.outgoing().map_or(1.0, rodio::Sink::volume);
        envelope * sink
    }

    pub(crate) fn ramp_handover(&mut self, length: Duration, playing: f32) {
        if let Some(control) = self.source.envelopes.outgoing.as_mut() {
            let frames = control.frames(length);
            control.ramp(Ramp {
                from: 1.0,
                to: 0.0,
                curve: Curve::EqualPowerOut,
                frames,
            });
        }
        if let Some(control) = self.source.envelopes.primary.as_mut() {
            let frames = control.frames(length);
            control.ramp(Ramp {
                from: 0.0,
                to: 1.0,
                curve: Curve::EqualPowerIn,
                frames,
            });
        }
        if let Some(sink) = self.primary() {
            sink.set_volume(playing);
        }
    }

    pub(crate) fn primary(&self) -> Option<&rodio::Sink> {
        self.output.as_ref().map(|output| &output.sink)
    }

    pub(crate) fn outgoing(&self) -> Option<&rodio::Sink> {
        self.output
            .as_ref()
            .and_then(|output| output.outgoing.as_ref())
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        self.output.iter().flat_map(Output::sinks)
    }

    pub(crate) fn list_devices(&self) {
        self.source.list_devices();
    }
}

fn slot_for(envelopes: &Envelopes, ticket: Ticket) -> Option<Slot> {
    if envelopes
        .primary
        .as_ref()
        .is_some_and(|control| control.ticket() == ticket)
    {
        Some(Slot::Primary)
    } else if envelopes
        .incoming
        .as_ref()
        .is_some_and(|control| control.ticket() == ticket)
    {
        Some(Slot::Incoming)
    } else if envelopes
        .outgoing
        .as_ref()
        .is_some_and(|control| control.ticket() == ticket)
    {
        Some(Slot::Outgoing)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::AudioFailure;
    use rodio::{Source, source::SineWave};
    use rstest::rstest;

    use crate::{
        deck::{Deck, DeckEvent, Ticket, envelope::envelope, source::PreloadRequest},
        engine::effect::{EngineMessage, Slot},
        error::AudioError,
        tap,
    };

    fn deck_with_no_output() -> Deck {
        let (spectrum, _spectrum_tap) = tap::new_tap();
        Deck::new(spectrum)
    }

    fn no_pending(_deck: &mut Deck) {}

    fn decode_pending(deck: &mut Deck) {
        deck.spawn_decode(PathBuf::from("/a"));
    }

    fn preload_pending(deck: &mut Deck) {
        deck.start_preload(PreloadRequest::Gapless(PathBuf::from("/b")));
    }

    fn decode_error() -> Result<crate::deck::source::TrackDecoder, AudioError> {
        Err(AudioError::Decode {
            path: PathBuf::from("/a"),
            source: rodio::decoder::DecoderError::UnrecognizedFormat,
        })
    }

    enum Expected {
        Nothing,
        DecodeError,
        PreloadError,
        OutputLost,
    }

    fn assert_expected(messages: &[EngineMessage], expected: &Expected) {
        match expected {
            Expected::Nothing => assert!(messages.is_empty()),
            Expected::DecodeError => assert!(matches!(
                messages,
                [EngineMessage::Decoded(Err(AudioFailure::Decode { .. }))]
            )),
            Expected::PreloadError => assert!(matches!(
                messages,
                [EngineMessage::Preloaded(Err(AudioFailure::Preload { .. }))]
            )),
            Expected::OutputLost => assert!(matches!(
                messages,
                [EngineMessage::Failed(AudioFailure::OutputLost { .. })]
            )),
        }
    }

    #[rstest]
    #[case::stale_decode_ticket(
        decode_pending,
        DeckEvent::Decoded { ticket: Ticket::default(), outcome: decode_error() },
        Expected::Nothing
    )]
    #[case::current_decode_error(
        decode_pending,
        DeckEvent::Decoded { ticket: Ticket::default().next(), outcome: decode_error() },
        Expected::DecodeError
    )]
    #[case::stale_preload_ticket(
        preload_pending,
        DeckEvent::Preloaded { ticket: Ticket::default(), outcome: decode_error() },
        Expected::Nothing
    )]
    #[case::preload_error(
        preload_pending,
        DeckEvent::Preloaded { ticket: Ticket::default().next(), outcome: decode_error() },
        Expected::PreloadError
    )]
    #[case::panicked_worker(
        decode_pending,
        DeckEvent::Decoded {
            ticket: Ticket::default().next(),
            outcome: Err(AudioError::WorkerPanicked { path: PathBuf::from("/a") }),
        },
        Expected::DecodeError
    )]
    #[case::stream_fault(
        no_pending,
        DeckEvent::Fault(rodio::cpal::StreamError::DeviceNotAvailable),
        Expected::OutputLost
    )]
    fn a_landed_event_becomes_a_message(
        #[case] pending: fn(&mut Deck),
        #[case] event: DeckEvent,
        #[case] expected: Expected,
    ) {
        let mut deck = deck_with_no_output();
        pending(&mut deck);
        let messages = deck.landed(event);
        assert_expected(&messages, &expected);
    }

    fn tone(millis: u64) -> impl Source {
        SineWave::new(440.0).take_duration(Duration::from_millis(millis))
    }

    #[test]
    fn track_finished_primary() {
        let mut deck = deck_with_no_output();
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(1), Ticket::default().next(), wake);
        for _ in source {}
        let ticket = control.ticket();
        deck.source.envelopes.primary = Some(control);

        let messages = deck.landed(DeckEvent::Track(ticket));
        assert!(matches!(
            messages.as_slice(),
            [EngineMessage::Finished(Slot::Primary)]
        ));
    }

    #[test]
    fn track_unknown_ticket() {
        let mut deck = deck_with_no_output();
        let messages = deck.landed(DeckEvent::Track(Ticket::default()));
        assert!(messages.is_empty());
    }
}
