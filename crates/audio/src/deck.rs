pub(crate) mod envelope;
pub(crate) mod event;
pub mod job;
pub(crate) mod output;
pub(crate) mod source;

use std::{path::PathBuf, time::Duration};

use crossbeam_channel::Sender;
use kernel::{
    AudioError,
    Playback,
    domain::{OutputDevice, Revision, Speed},
};

use crate::{
    deck::{
        envelope::{EnvelopeControl, Ramp, envelope},
        output::{Output, open_output_stream},
        source::{PreloadMode, TrackDecoding, TrackSource},
    },
    device::OutputLoss,
    engine::{
        effect::{AudioMessage, PreloadKind},
        phase::CurrentTrack,
    },
    error::device_error,
    gain::Gain,
    tap::Handoff,
};

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceOpened {
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
    source: TrackDecoding,
    sender: Sender<AudioMessage>,
    spectrum: Handoff,
    lost: OutputLoss,
}

impl Deck {
    pub(crate) fn new(spectrum: Handoff, sender: Sender<AudioMessage>) -> Self {
        let source = TrackDecoding::new(sender.clone());
        Self {
            output: None,
            source,
            sender,
            spectrum,
            lost: OutputLoss::default(),
        }
    }

    pub(crate) fn resend_lost(&self) {
        self.lost.resend(&self.sender);
    }

    pub(crate) fn stage(&mut self, track: TrackSource) {
        self.source.stage(track);
    }

    pub(crate) fn attach(&mut self, track: TrackSource) -> Option<AudioMessage> {
        let (path, mode) = self.source.take_preloading()?;
        let output = self.output.as_mut()?;
        let total = track.total();
        let (wrapped, control) =
            envelope(track.source, track.revision, self.sender.clone());
        let preload = match mode {
            PreloadMode::Gapless => {
                output.append(wrapped);
                self.source.envelopes.queued = Some(control);
                PreloadKind::Gapless(path)
            }
            PreloadMode::Crossfade { gain, speed } => {
                output.stage(wrapped, speed);
                self.source.envelopes.incoming = Some(control);
                PreloadKind::Crossfade(CurrentTrack { total, gain, path })
            }
        };
        Some(AudioMessage::Preloaded(preload))
    }

    pub(crate) fn take_signals(&self, revision: Revision) -> Option<AudioMessage> {
        let signals = self.source.envelopes.holding(revision)?.take_signals();
        let role = self.source.envelopes.role(revision)?;
        Some(AudioMessage::SignalsTaken { role, signals })
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
        speed: Speed,
    ) -> Result<DeviceOpened, AudioError> {
        let opened = open_output_stream(device, &self.sender, &self.lost)
            .map_err(device_error)?;
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

    pub(crate) fn start_decode(&mut self) {
        self.source.start_decode();
    }

    pub(crate) fn start_preload(&mut self, path: PathBuf, mode: PreloadMode) {
        self.source.start_preload(path, mode);
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

    pub(crate) fn retire_primary(&mut self, speed: Speed) {
        if let Some(output) = self.output.as_mut() {
            output.retire_sink(speed);
        }
        self.source.envelopes.outgoing = self.source.envelopes.primary.take();
    }

    pub(crate) fn swap_primary(&mut self, speed: Speed) {
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

    pub(crate) fn crossfade(&mut self, length: Duration, incoming: Gain) {
        let envelopes = &mut self.source.envelopes;
        fade(
            envelopes.primary.as_mut(),
            envelopes.incoming.as_mut(),
            length,
        );
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let Some(preload) = output.incoming.as_ref() else {
            return;
        };
        preload.set_volume(incoming.amplitude());
        preload.play();
    }

    pub(crate) fn cancel_crossfade(&mut self) {
        if let Some(control) = self.source.envelopes.primary.as_mut() {
            control.ramp(Ramp::hold(Gain::UNITY));
        }
        if let Some(control) = self.source.envelopes.incoming.as_mut() {
            control.ramp(Ramp::hold(Gain::SILENCE));
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

    pub(crate) fn ramp_handover(&mut self, length: Duration, playing: Gain) {
        let envelopes = &mut self.source.envelopes;
        fade(
            envelopes.outgoing.as_mut(),
            envelopes.primary.as_mut(),
            length,
        );
        if let Some(sink) = self.primary() {
            sink.set_volume(playing.amplitude());
        }
    }

    pub(crate) fn primary(&self) -> Option<&rodio::Sink> {
        self.output.as_ref().map(|output| &output.primary)
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        self.output.iter().flat_map(Output::sinks)
    }
}

fn fade(
    out: Option<&mut EnvelopeControl>,
    into: Option<&mut EnvelopeControl>,
    length: Duration,
) {
    if let Some(control) = out {
        let frames = control.frames(length);
        control.ramp(Ramp::fade_out(frames));
    }
    if let Some(control) = into {
        let frames = control.frames(length);
        control.ramp(Ramp::fade_in(frames));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::Revision;
    use rodio::{Source, source::SineWave};

    use crate::{
        deck::{
            Deck,
            envelope::{Ramp, Signals, envelope},
        },
        engine::effect::{AudioMessage, SinkRole},
        tap,
    };

    fn deck_with_no_output() -> Deck {
        let (spectrum, _spectrum_tap) = tap::new_tap();
        let (sender, _heard) = crossbeam_channel::bounded(64);
        Deck::new(spectrum, sender)
    }

    fn tone(millis: u64) -> impl Source {
        SineWave::new(440.0).take_duration(Duration::from_millis(millis))
    }

    #[test]
    fn a_finished_primary_sink_reports_the_track_finished() {
        let mut deck = deck_with_no_output();
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(1), Revision::default().next(), wake);
        for _ in source {}
        let revision = control.revision();
        deck.source.envelopes.primary = Some(control);

        let answer = deck.take_signals(revision);
        assert!(matches!(
            answer,
            Some(AudioMessage::SignalsTaken { role: SinkRole::Primary, signals })
                if signals.contains(Signals::FINISHED)
        ));
        assert!(matches!(
            deck.take_signals(revision),
            Some(AudioMessage::SignalsTaken { signals, .. }) if signals == Signals::default()
        ));
    }

    #[test]
    fn taking_signals_for_an_unknown_ticket_answers_nothing() {
        let deck = deck_with_no_output();
        assert!(deck.take_signals(Revision::default()).is_none());
    }

    #[test]
    fn a_faded_outgoing_track_keeps_its_ramped_signal_after_a_row_pick() {
        let mut deck = deck_with_no_output();
        let decoded = Revision::default().next();
        let preloaded = decoded.next();
        let (wake, _heard) = crossbeam_channel::bounded(8);
        let (source, mut outgoing) = envelope(tone(100), preloaded, wake.clone());
        outgoing.ramp(Ramp::fade_out(outgoing.frames(Duration::from_millis(10))));
        for _ in source {}
        let revision = outgoing.revision();
        let (_source, primary) = envelope(tone(100), decoded, wake);
        deck.source.envelopes.outgoing = Some(outgoing);
        deck.source.envelopes.primary = Some(primary);

        assert!(matches!(
            deck.take_signals(revision),
            Some(AudioMessage::SignalsTaken { role: SinkRole::Outgoing, signals })
                if signals.contains(Signals::RAMPED)
        ));
    }
}
