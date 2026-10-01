pub(crate) mod envelope;
mod event;
pub(crate) mod output;
pub(crate) mod source;
mod worker;

use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
pub(crate) use event::{DeckEvent, Ticket};
use kernel::{AudioError, AudioEvent, Playback, domain::OutputDevice};
use rodio::Source;

use crate::{
    deck::{
        envelope::{Curve, Envelopes, Ramp, Signals, envelope},
        output::{Output, open_output_stream},
        source::{DecodeResult, Decoding, PreloadRequest},
    },
    engine::{
        effect::{EngineMessage, Preload, SinkRole},
        phase::CurrentTrack,
    },
    error::{device_error, output_lost, preload_error},
    tap::Handoff,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DeviceOpened {
    pub(crate) device: OutputDevice,
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
    pub(crate) opened: DeviceChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceChoice {
    FellBack,
    Requested,
}

pub(crate) struct Deck {
    pub(crate) output: Option<Output>,
    source: Decoding,
    events: Vec<AudioEvent>,
    refused: Vec<&'static str>,
    wake: Sender<DeckEvent>,
    heard: Receiver<DeckEvent>,
    spectrum: Handoff,
}

impl Deck {
    pub(crate) fn new(spectrum: Handoff) -> Self {
        let (wake, heard) = crossbeam_channel::bounded(64);
        let source = Decoding::new(wake.clone());
        Self {
            output: None,
            source,
            events: Vec::new(),
            refused: Vec::new(),
            wake,
            heard,
            spectrum,
        }
    }

    pub(crate) fn send(&mut self, event: AudioEvent) {
        self.events.push(event);
    }

    pub(crate) fn refuse(&mut self, input: &'static str) {
        self.refused.push(input);
    }

    pub(crate) fn drain_refused(&mut self) -> std::vec::Drain<'_, &'static str> {
        self.refused.drain(..)
    }

    pub(crate) fn drain_events(&mut self) -> std::vec::Drain<'_, AudioEvent> {
        self.events.drain(..)
    }

    pub(crate) fn events(&self) -> &Receiver<DeckEvent> {
        &self.heard
    }

    pub(crate) fn messages_for(&mut self, event: DeckEvent) -> Vec<EngineMessage> {
        match event {
            DeckEvent::OutputLost(error) => {
                vec![EngineMessage::Failed(output_lost(&error))]
            }
            DeckEvent::Decoded { ticket, outcome } => self
                .source
                .accept_decode(ticket, outcome)
                .map(EngineMessage::Decoded)
                .into_iter()
                .collect(),
            DeckEvent::Preloaded { ticket, outcome } => {
                self.preloaded(ticket, outcome).into_iter().collect()
            }
            DeckEvent::DevicesListed(devices) => {
                vec![EngineMessage::DevicesListed(devices)]
            }
            DeckEvent::Track(ticket) => self.track_events(ticket),
        }
    }

    fn preloaded(
        &mut self,
        ticket: Ticket,
        outcome: DecodeResult,
    ) -> Option<EngineMessage> {
        let request = self.source.take_preloading(ticket)?;
        let source = match outcome {
            Ok(source) => source,
            Err(error) => {
                return Some(EngineMessage::Preloaded(Err(preload_error(&error))));
            }
        };
        let output = self.output.as_mut()?;
        let total = source.total_duration();
        let (wrapped, control) = envelope(source, ticket, self.wake.clone());
        let preload = match request {
            PreloadRequest::Gapless(path) => {
                output.append(wrapped);
                self.source.envelopes.queued = Some(control);
                Preload::Gapless(path)
            }
            PreloadRequest::Crossfade { path, gain, speed } => {
                output.stage(wrapped, speed.get());
                self.source.envelopes.incoming = Some(control);
                Preload::Crossfade(CurrentTrack { total, gain, path })
            }
        };
        Some(EngineMessage::Preloaded(Ok(preload)))
    }

    fn track_events(&self, ticket: Ticket) -> Vec<EngineMessage> {
        let Some(control) = self.source.envelopes.holding(ticket) else {
            return Vec::new();
        };
        let flags = control.take_signals();
        let role = sink_role_for(&self.source.envelopes, ticket);
        [
            (flags.contains(Signals::CUED) && matches!(role, Some(SinkRole::Primary)))
                .then_some(EngineMessage::Cued),
            role.filter(|_| flags.contains(Signals::RAMPED))
                .map(EngineMessage::Ramped),
            role.filter(|_| flags.contains(Signals::FINISHED))
                .map(EngineMessage::Finished),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    pub(crate) fn advance(&mut self) {
        self.source.envelopes.primary = self.source.envelopes.queued.take();
    }

    pub(crate) fn playhead(&self) -> Option<Duration> {
        if let Some(control) = self.source.envelopes.primary.as_ref() {
            return Some(control.position());
        }
        self.output.as_ref().map(|output| output.primary.get_pos())
    }

    pub(crate) fn silence(&mut self) {
        self.drop_preload();
        self.output = None;
        self.source.clear_staged();
    }

    pub(crate) fn open(
        &mut self,
        device: OutputDevice,
        speed: f32,
    ) -> Result<DeviceOpened, AudioError> {
        let opened = open_output_stream(device, &self.wake).map_err(device_error)?;
        let (position, playback) = self
            .output
            .as_ref()
            .map_or((Duration::ZERO, Playback::Playing), Output::at);
        drop(self.output.take());
        let mut output = Output::with_stream(opened.stream, speed);
        output.listen(&self.spectrum);
        self.output = Some(output);
        Ok(DeviceOpened {
            device: opened.device,
            position,
            playback,
            opened: opened.opened,
        })
    }

    pub(crate) fn start_decode(
        &mut self,
        path: std::path::PathBuf,
    ) -> Option<EngineMessage> {
        self.source
            .start_decode(path)
            .map(|error| EngineMessage::Decoded(Err(error)))
    }

    pub(crate) fn start_preload(
        &mut self,
        request: PreloadRequest,
    ) -> Option<EngineMessage> {
        self.source
            .start_preload(request)
            .map(|error| EngineMessage::Preloaded(Err(error)))
    }

    pub(crate) fn drop_preload(&mut self) {
        self.source.drop_preload();
        if let Some(output) = self.output.as_mut() {
            output.incoming = None;
        }
    }

    pub(crate) fn clear_staged(&mut self) {
        self.source.clear_staged();
    }

    pub(crate) fn append_staged(&mut self) {
        if let Some(output) = self.output.as_ref() {
            self.source.append_staged(output);
        }
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
        let Some(preload) = output.incoming.as_ref() else {
            return;
        };
        preload.set_volume(incoming);
        preload.play();
    }

    pub(crate) fn cancel_crossfade(&mut self) {
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
            .and_then(|output| output.incoming.as_ref())
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
        self.output.as_ref().map(|output| &output.primary)
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        self.output.iter().flat_map(Output::sinks)
    }

    pub(crate) fn list_devices(&self) {
        self.source.list_devices();
    }
}

fn sink_role_for(envelopes: &Envelopes, ticket: Ticket) -> Option<SinkRole> {
    if envelopes
        .primary
        .as_ref()
        .is_some_and(|control| control.ticket() == ticket)
    {
        Some(SinkRole::Primary)
    } else if envelopes
        .incoming
        .as_ref()
        .is_some_and(|control| control.ticket() == ticket)
    {
        Some(SinkRole::Incoming)
    } else if envelopes
        .outgoing
        .as_ref()
        .is_some_and(|control| control.ticket() == ticket)
    {
        Some(SinkRole::Outgoing)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::AudioError;
    use rodio::{Source, source::SineWave};
    use rstest::rstest;

    use crate::{
        deck::{Deck, DeckEvent, Ticket, envelope::envelope, source::PreloadRequest},
        engine::effect::{EngineMessage, SinkRole},
        error::Error,
        tap,
    };

    fn deck_with_no_output() -> Deck {
        let (spectrum, _spectrum_tap) = tap::new_tap();
        Deck::new(spectrum)
    }

    fn no_pending(_deck: &mut Deck) {}

    fn decode_pending(deck: &mut Deck) {
        deck.start_decode(PathBuf::from("/a"));
    }

    fn preload_pending(deck: &mut Deck) {
        deck.start_preload(PreloadRequest::Gapless(PathBuf::from("/b")));
    }

    fn decode_error() -> Result<crate::deck::source::TrackDecoder, Error> {
        Err(Error::Decode {
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
                [EngineMessage::Decoded(Err(AudioError::Decode { .. }))]
            )),
            Expected::PreloadError => assert!(matches!(
                messages,
                [EngineMessage::Preloaded(Err(AudioError::Preload { .. }))]
            )),
            Expected::OutputLost => assert!(matches!(
                messages,
                [EngineMessage::Failed(AudioError::OutputLost { .. })]
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
            outcome: Err(Error::WorkerPanicked { path: PathBuf::from("/a") }),
        },
        Expected::DecodeError
    )]
    #[case::stream_error(
        no_pending,
        DeckEvent::OutputLost(rodio::cpal::StreamError::DeviceNotAvailable),
        Expected::OutputLost
    )]
    fn a_landed_event_becomes_a_message(
        #[case] pending: fn(&mut Deck),
        #[case] event: DeckEvent,
        #[case] expected: Expected,
    ) {
        let mut deck = deck_with_no_output();
        pending(&mut deck);
        let messages = deck.messages_for(event);
        assert_expected(&messages, &expected);
    }

    fn tone(millis: u64) -> impl Source {
        SineWave::new(440.0).take_duration(Duration::from_millis(millis))
    }

    #[test]
    fn a_finished_primary_sink_reports_the_track_finished() {
        let mut deck = deck_with_no_output();
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(1), Ticket::default().next(), wake);
        for _ in source {}
        let ticket = control.ticket();
        deck.source.envelopes.primary = Some(control);

        let messages = deck.messages_for(DeckEvent::Track(ticket));
        assert!(matches!(
            messages.as_slice(),
            [EngineMessage::Finished(SinkRole::Primary)]
        ));
    }

    #[test]
    fn a_track_event_with_an_unknown_ticket_gives_no_message() {
        let mut deck = deck_with_no_output();
        let messages = deck.messages_for(DeckEvent::Track(Ticket::default()));
        assert!(messages.is_empty());
    }
}
