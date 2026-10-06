pub(crate) mod envelope;
pub(crate) mod event;
pub mod job;
pub(crate) mod output;
pub(crate) mod source;

use std::time::Duration;

use crossbeam_channel::Sender;
use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, revision::Revision, speed::Speed},
};

use crate::{
    deck::{
        envelope::{EnvelopeControl, Ramp, envelope},
        output::{Fader, Output},
        source::{DecodedTrack, PreloadMode},
    },
    device::{OutputLoss, open_stream},
    engine::message::{AudioMessage, DeviceOpened, EngineMessage},
    error::DeviceError,
    gain::Gain,
    tap::SpectrumBuffers,
};

pub(crate) struct Deck {
    stream: Option<rodio::OutputStream>,
    pub(crate) output: Option<Output>,
    staged_track: Option<DecodedTrack>,
    callback_sender: Sender<AudioMessage>,
    spectrum_buffers: SpectrumBuffers,
    output_loss: OutputLoss,
}

impl Deck {
    pub(crate) fn new(
        spectrum_buffers: SpectrumBuffers,
        callback_sender: Sender<AudioMessage>,
    ) -> Self {
        Self {
            stream: None,
            output: None,
            staged_track: None,
            callback_sender,
            spectrum_buffers,
            output_loss: OutputLoss::default(),
        }
    }

    pub(crate) fn resend_output_loss(&self) {
        self.output_loss.resend(&self.callback_sender);
    }

    pub(crate) fn stage(&mut self, decoded_track: DecodedTrack) {
        self.staged_track = Some(decoded_track);
    }

    pub(crate) fn attach(
        &mut self,
        decoded_track: DecodedTrack,
        preload_mode: PreloadMode,
    ) -> Option<AudioMessage> {
        let output = self.output.as_mut()?;
        let duration = decoded_track.duration();
        let (wrapped, control) = envelope(
            decoded_track.decoder,
            decoded_track.revision,
            self.callback_sender.clone(),
        );
        match preload_mode {
            PreloadMode::Gapless => {
                output.append(wrapped);
                output.incoming_control = Some(control);
            }
            PreloadMode::Crossfade(speed) => {
                let sink = output.attach_incoming(wrapped, speed);
                output.incoming_fader = Some(Fader { sink, control });
            }
        }
        Some(
            EngineMessage::Attached {
                revision: decoded_track.revision,
                preload_mode,
                duration,
            }
            .into(),
        )
    }

    pub(crate) fn take_signals(&self, revision: Revision) -> Option<AudioMessage> {
        let output = self.output.as_ref()?;
        let (role, control) = output.holder(revision)?;
        Some(AudioMessage::SignalsTaken {
            role,
            signals: control.take_signals(),
        })
    }

    pub(crate) fn advance(&mut self) -> Option<AudioMessage> {
        let output = self.output.as_mut()?;
        output.current_control = output.incoming_control.take();
        let revision = output.current_control.as_ref()?.revision();
        self.take_signals(revision)
    }

    pub(crate) fn position(&self) -> Option<Duration> {
        let output = self.output.as_ref()?;
        Some(
            output
                .current_control
                .as_ref()
                .map_or_else(|| output.current.get_pos(), EnvelopeControl::position),
        )
    }

    pub(crate) fn silence(&mut self) {
        self.drop_preload();
        self.output = None;
        self.stream = None;
        self.staged_track = None;
    }

    pub(crate) fn open(
        &mut self,
        device: OutputDevice,
        speed: Speed,
    ) -> Result<DeviceOpened, DeviceError> {
        let stream = open_stream(&device, &self.callback_sender, &self.output_loss)?;
        self.drop_preload();
        let (position, playback) = self
            .output
            .as_ref()
            .map_or((Duration::ZERO, Playback::Playing), Output::position);
        drop(self.output.take());
        self.output = Some(Output::with_stream(&stream, speed, &self.spectrum_buffers));
        self.stream = Some(stream);
        Ok(DeviceOpened {
            device,
            position,
            playback,
        })
    }

    pub(crate) fn drop_preload(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.incoming_fader = None;
        }
    }

    pub(crate) fn clear_staged(&mut self) {
        self.staged_track = None;
    }

    pub(crate) fn append_staged(&mut self) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let Some(DecodedTrack { revision, decoder }) = self.staged_track.take() else {
            return;
        };
        let (wrapped, control) =
            envelope(decoder, revision, self.callback_sender.clone());
        output.append(wrapped);
        output.current_control = Some(control);
    }

    pub(crate) fn promote(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.promote();
        }
    }

    pub(crate) fn retire_current(&mut self, speed: Speed) {
        if let Some(output) = self.output.as_mut() {
            output.retire_current(speed);
        }
    }

    pub(crate) fn swap_current(&mut self, speed: Speed) {
        if let Some(output) = self.output.as_mut() {
            output.swap_current(speed);
        }
    }

    pub(crate) fn set_fade_start(&mut self, fade_start: Option<Duration>) {
        let Some(control) = self
            .output
            .as_mut()
            .and_then(|output| output.current_control.as_mut())
        else {
            return;
        };
        control.set_fade_start(fade_start);
    }

    pub(crate) fn crossfade(&mut self, duration: Duration, incoming: Gain) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        fade(
            output.current_control.as_mut(),
            output
                .incoming_fader
                .as_mut()
                .map(|fader| &mut fader.control),
            duration,
        );
        if let Some(incoming_fader) = output.incoming_fader.as_ref() {
            incoming_fader.sink.set_volume(incoming.amplitude());
            incoming_fader.sink.play();
        }
    }

    pub(crate) fn cancel_crossfade(&mut self) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        if let Some(control) = output.current_control.as_mut() {
            control.ramp(Ramp::hold(Gain::UNITY));
        }
        if let Some(incoming_fader) = output.incoming_fader.as_mut() {
            incoming_fader.control.ramp(Ramp::hold(Gain::SILENCE));
            incoming_fader.sink.pause();
        }
    }

    pub(crate) fn drop_outgoing(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.outgoing_fader = None;
        }
    }

    pub(crate) fn ramp_handover(&mut self, duration: Duration, current: Gain) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        fade(
            output
                .outgoing_fader
                .as_mut()
                .map(|fader| &mut fader.control),
            output.current_control.as_mut(),
            duration,
        );
        output.current.set_volume(current.amplitude());
    }

    pub(crate) fn current(&self) -> Option<&rodio::Sink> {
        self.output.as_ref().map(|output| &output.current)
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        self.output.iter().flat_map(Output::sinks)
    }
}

fn fade(
    outgoing: Option<&mut EnvelopeControl>,
    incoming: Option<&mut EnvelopeControl>,
    duration: Duration,
) {
    if let Some(control) = outgoing {
        let frames = control.frames(duration);
        control.ramp(Ramp::fade_out(frames));
    }
    if let Some(control) = incoming {
        let frames = control.frames(duration);
        control.ramp(Ramp::fade_in(frames));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{io::Write, time::Duration};

    use kernel::domain::{revision::Revision, speed::Speed};
    use rodio::{Source, source::SineWave};
    use rstest::rstest;

    use crate::{
        deck::{
            Deck,
            envelope::{EnvelopeControl, Ramp, envelope},
            output::{Output, tests::detached_output},
            source::{DecodedTrack, PreloadMode, decode},
        },
        engine::{
            message::{AudioMessage, EngineMessage, Signals, SinkRole},
            tests::assert_same,
        },
        tap,
    };

    type Slots = [Option<Revision>; 5];

    fn wav_bytes() -> Vec<u8> {
        let samples = [0_u8; 16];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&52_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&8000_u32.to_le_bytes());
        bytes.extend_from_slice(&16000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&samples);
        bytes
    }

    pub(crate) fn track(revision: Revision) -> DecodedTrack {
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        file.write_all(&wav_bytes()).unwrap();
        DecodedTrack {
            revision,
            decoder: decode(file.path()).unwrap(),
        }
    }

    fn control(revision: Revision) -> EnvelopeControl {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        envelope(tone(1), revision, callback_sender).1
    }

    fn staged_revision() -> Revision {
        Revision::default().next()
    }

    fn incoming_revision() -> Revision {
        staged_revision().next()
    }

    fn loaded_deck() -> Deck {
        let mut deck = deck_with_detached_output();
        deck.stage(track(staged_revision()));
        output(&mut deck).incoming_control = Some(control(incoming_revision()));
        deck
    }

    fn output(deck: &mut Deck) -> &mut Output {
        deck.output.as_mut().unwrap()
    }

    fn slots(deck: &Deck) -> Slots {
        let output = deck.output.as_ref();
        [
            output
                .and_then(|output| output.current_control.as_ref())
                .map(EnvelopeControl::revision),
            output
                .and_then(|output| output.incoming_control.as_ref())
                .map(EnvelopeControl::revision),
            output
                .and_then(|output| output.incoming_fader.as_ref())
                .map(|fader| fader.control.revision()),
            output
                .and_then(|output| output.outgoing_fader.as_ref())
                .map(|fader| fader.control.revision()),
            deck.staged_track.as_ref().map(|staged| staged.revision),
        ]
    }

    fn attach_gapless(deck: &mut Deck) -> Option<AudioMessage> {
        deck.attach(track(Revision::default()), PreloadMode::Gapless)
    }

    fn attach_crossfade(deck: &mut Deck) -> Option<AudioMessage> {
        deck.attach(
            track(Revision::default()),
            PreloadMode::Crossfade(Speed::default()),
        )
    }

    fn attached(preload_mode: PreloadMode) -> Option<AudioMessage> {
        Some(
            EngineMessage::Attached {
                revision: Revision::default(),
                preload_mode,
                duration: track(Revision::default()).duration(),
            }
            .into(),
        )
    }

    fn unsignalled_current() -> Option<AudioMessage> {
        Some(AudioMessage::SignalsTaken {
            role: SinkRole::Current,
            signals: Signals::default(),
        })
    }

    fn append_staged(deck: &mut Deck) -> Option<AudioMessage> {
        deck.append_staged();
        None
    }

    fn silence(deck: &mut Deck) -> Option<AudioMessage> {
        deck.silence();
        None
    }

    fn clear_staged(deck: &mut Deck) -> Option<AudioMessage> {
        deck.clear_staged();
        None
    }

    #[rstest]
    #[case::attach_gapless_replaces_the_incoming_control(
        attach_gapless,
        [None, Some(Revision::default()), None, None, Some(staged_revision())],
        attached(PreloadMode::Gapless)
    )]
    #[case::attach_crossfade_fills_the_incoming_slot(
        attach_crossfade,
        [
            None,
            Some(incoming_revision()),
            Some(Revision::default()),
            None,
            Some(staged_revision())
        ],
        attached(PreloadMode::Crossfade(Speed::default()))
    )]
    #[case::advance_moves_incoming_to_current(
        Deck::advance,
        [Some(incoming_revision()), None, None, None, Some(staged_revision())],
        unsignalled_current()
    )]
    #[case::append_staged_moves_the_staged_track_to_current(
        append_staged,
        [Some(staged_revision()), Some(incoming_revision()), None, None, None],
        None
    )]
    #[case::silence_drops_every_slot(silence, [None, None, None, None, None], None)]
    #[case::clear_staged_drops_only_the_staged_track(
        clear_staged,
        [None, Some(incoming_revision()), None, None, None],
        None
    )]
    fn a_deck_operation_changes_only_its_own_slots(
        #[case] operation: fn(&mut Deck) -> Option<AudioMessage>,
        #[case] expected: Slots,
        #[case] audio_message: Option<AudioMessage>,
    ) {
        let mut deck = loaded_deck();
        assert_same(operation(&mut deck), audio_message);
        assert_eq!(slots(&deck), expected);
    }

    pub(crate) fn deck_with_detached_output() -> Deck {
        let (spectrum_buffers, _spectrum_tap) = tap::spectrum_channel();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(64);
        let mut deck = Deck::new(spectrum_buffers, callback_sender);
        deck.output = Some(detached_output());
        deck
    }

    pub(crate) fn tone(millis: u64) -> impl Source {
        SineWave::new(440.0).take_duration(Duration::from_millis(millis))
    }

    #[test]
    fn a_finished_current_sink_reports_the_track_finished() {
        let mut deck = deck_with_detached_output();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, control) =
            envelope(tone(1), Revision::default().next(), callback_sender);
        for _ in source {}
        let revision = control.revision();
        output(&mut deck).current_control = Some(control);

        let answer = deck.take_signals(revision);
        assert!(matches!(
            answer,
            Some(AudioMessage::SignalsTaken { role: SinkRole::Current, signals })
                if signals.contains(Signals::FINISHED)
        ));
        assert!(matches!(
            deck.take_signals(revision),
            Some(AudioMessage::SignalsTaken { signals, .. }) if signals == Signals::default()
        ));
    }

    #[test]
    fn a_gapless_incoming_track_keeps_its_signals_until_it_is_current() {
        let mut deck = deck_with_detached_output();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, control) =
            envelope(tone(1), Revision::default().next(), callback_sender);
        for _ in source {}
        let revision = control.revision();
        output(&mut deck).incoming_control = Some(control);

        assert!(deck.take_signals(revision).is_none());
        assert!(matches!(
            deck.advance(),
            Some(AudioMessage::SignalsTaken { role: SinkRole::Current, signals })
                if signals.contains(Signals::FINISHED)
        ));
    }

    #[test]
    fn taking_signals_for_an_unknown_revision_answers_nothing() {
        let deck = deck_with_detached_output();
        assert!(deck.take_signals(Revision::default()).is_none());
    }

    #[test]
    fn a_faded_outgoing_track_keeps_its_ramped_signal_after_a_row_pick() {
        let mut deck = deck_with_detached_output();
        let decoded = Revision::default().next();
        let preloaded = decoded.next();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(8);
        let (source, mut outgoing) =
            envelope(tone(100), preloaded, callback_sender.clone());
        outgoing.ramp(Ramp::fade_out(outgoing.frames(Duration::from_millis(10))));
        for _ in source {}
        let revision = outgoing.revision();
        let (_source, current) = envelope(tone(100), decoded, callback_sender);
        let output = output(&mut deck);
        output.current_control = Some(outgoing);
        output.retire_current(Speed::default());
        output.current_control = Some(current);

        assert!(matches!(
            deck.take_signals(revision),
            Some(AudioMessage::SignalsTaken { role: SinkRole::Outgoing, signals })
                if signals.contains(Signals::RAMPED)
        ));
    }
}
