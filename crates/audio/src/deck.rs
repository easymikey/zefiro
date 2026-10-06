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
    message::AudioError,
};

use crate::{
    deck::{
        envelope::{EnvelopeControl, Ramp, envelope},
        output::{Fader, Output, open_output_stream},
        source::{PreloadMode, TrackSource},
    },
    device::OutputLoss,
    engine::message::{AudioMessage, DeviceOpened, EngineMessage},
    error::device_error,
    gain::Gain,
    tap::Handoff,
};

pub(crate) struct Deck {
    stream: Option<rodio::OutputStream>,
    pub(crate) output: Option<Output>,
    staged: Option<TrackSource>,
    sender: Sender<AudioMessage>,
    spectrum: Handoff,
    lost: OutputLoss,
}

impl Deck {
    pub(crate) fn new(spectrum: Handoff, sender: Sender<AudioMessage>) -> Self {
        Self {
            stream: None,
            output: None,
            staged: None,
            sender,
            spectrum,
            lost: OutputLoss::default(),
        }
    }

    pub(crate) fn resend_lost(&self) {
        self.lost.resend(&self.sender);
    }

    pub(crate) fn stage(&mut self, track: TrackSource) {
        self.staged = Some(track);
    }

    pub(crate) fn attach(
        &mut self,
        track: TrackSource,
        preload_mode: PreloadMode,
    ) -> Option<AudioMessage> {
        let output = self.output.as_mut()?;
        let duration = track.total();
        let (wrapped, control) =
            envelope(track.source, track.revision, self.sender.clone());
        match preload_mode {
            PreloadMode::Gapless => {
                output.append(wrapped);
                output.queued_control = Some(control);
            }
            PreloadMode::Crossfade(speed) => {
                let sink = output.stage(wrapped, speed);
                output.incoming_fader = Some(Fader { sink, control });
            }
        }
        Some(
            EngineMessage::Attached {
                revision: track.revision,
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
        output.control = output.queued_control.take();
        let revision = output.control.as_ref()?.revision();
        self.take_signals(revision)
    }

    pub(crate) fn playhead(&self) -> Option<Duration> {
        let output = self.output.as_ref()?;
        Some(
            output
                .control
                .as_ref()
                .map_or_else(|| output.primary.get_pos(), EnvelopeControl::position),
        )
    }

    pub(crate) fn silence(&mut self) {
        self.drop_preload();
        self.output = None;
        self.stream = None;
        self.staged = None;
    }

    pub(crate) fn open(
        &mut self,
        device: OutputDevice,
        speed: Speed,
    ) -> Result<DeviceOpened, AudioError> {
        let opened = open_output_stream(device, &self.sender, &self.lost)
            .map_err(device_error)?;
        self.drop_preload();
        let (position, playback) = self
            .output
            .as_ref()
            .map_or((Duration::ZERO, Playback::Playing), Output::at);
        drop(self.output.take());
        self.output = Some(Output::with_stream(&opened.stream, speed, &self.spectrum));
        self.stream = Some(opened.stream);
        Ok(DeviceOpened {
            device: opened.device,
            position,
            playback,
            opened: opened.opened,
        })
    }

    pub(crate) fn drop_preload(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.incoming_fader = None;
        }
    }

    pub(crate) fn clear_staged(&mut self) {
        self.staged = None;
    }

    pub(crate) fn append_staged(&mut self) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let Some(TrackSource { revision, source }) = self.staged.take() else {
            return;
        };
        let (wrapped, control) = envelope(source, revision, self.sender.clone());
        output.append(wrapped);
        output.control = Some(control);
    }

    pub(crate) fn promote(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.promote();
        }
    }

    pub(crate) fn retire_primary(&mut self, speed: Speed) {
        if let Some(output) = self.output.as_mut() {
            output.retire_sink(speed);
        }
    }

    pub(crate) fn swap_primary(&mut self, speed: Speed) {
        if let Some(output) = self.output.as_mut() {
            output.swap_sink(speed);
        }
    }

    pub(crate) fn cue_primary(&mut self, cue: Option<Duration>) {
        let Some(control) = self
            .output
            .as_mut()
            .and_then(|output| output.control.as_mut())
        else {
            return;
        };
        control.cue(cue);
    }

    pub(crate) fn crossfade(&mut self, length: Duration, incoming: Gain) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        fade(
            output.control.as_mut(),
            output
                .incoming_fader
                .as_mut()
                .map(|fader| &mut fader.control),
            length,
        );
        if let Some(preload) = output.incoming_fader.as_ref() {
            preload.sink.set_volume(incoming.amplitude());
            preload.sink.play();
        }
    }

    pub(crate) fn cancel_crossfade(&mut self) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        if let Some(control) = output.control.as_mut() {
            control.ramp(Ramp::hold(Gain::UNITY));
        }
        if let Some(preload) = output.incoming_fader.as_mut() {
            preload.control.ramp(Ramp::hold(Gain::SILENCE));
            preload.sink.pause();
        }
    }

    pub(crate) fn drop_outgoing(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.outgoing_fader = None;
        }
    }

    pub(crate) fn ramp_handover(&mut self, length: Duration, playing: Gain) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        fade(
            output
                .outgoing_fader
                .as_mut()
                .map(|fader| &mut fader.control),
            output.control.as_mut(),
            length,
        );
        output.primary.set_volume(playing.amplitude());
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
            source::{PreloadMode, TrackSource, decode},
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

    pub(crate) fn track(revision: Revision) -> TrackSource {
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        file.write_all(&wav_bytes()).unwrap();
        TrackSource {
            revision,
            source: decode(file.path()).unwrap(),
        }
    }

    fn control(revision: Revision) -> EnvelopeControl {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        envelope(tone(1), revision, wake).1
    }

    fn staged_revision() -> Revision {
        Revision::default().next()
    }

    fn queued_revision() -> Revision {
        staged_revision().next()
    }

    fn loaded_deck() -> Deck {
        let mut deck = deck_with_detached_output();
        deck.stage(track(staged_revision()));
        output(&mut deck).queued_control = Some(control(queued_revision()));
        deck
    }

    fn output(deck: &mut Deck) -> &mut Output {
        deck.output.as_mut().unwrap()
    }

    fn slots(deck: &Deck) -> Slots {
        let output = deck.output.as_ref();
        [
            output
                .and_then(|output| output.control.as_ref())
                .map(EnvelopeControl::revision),
            output
                .and_then(|output| output.queued_control.as_ref())
                .map(EnvelopeControl::revision),
            output
                .and_then(|output| output.incoming_fader.as_ref())
                .map(|fader| fader.control.revision()),
            output
                .and_then(|output| output.outgoing_fader.as_ref())
                .map(|fader| fader.control.revision()),
            deck.staged.as_ref().map(|staged| staged.revision),
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
                duration: track(Revision::default()).total(),
            }
            .into(),
        )
    }

    fn unsignalled_primary() -> Option<AudioMessage> {
        Some(AudioMessage::SignalsTaken {
            role: SinkRole::Primary,
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
    #[case::attach_gapless_replaces_the_queued_slot(
        attach_gapless,
        [None, Some(Revision::default()), None, None, Some(staged_revision())],
        attached(PreloadMode::Gapless)
    )]
    #[case::attach_crossfade_fills_the_incoming_slot(
        attach_crossfade,
        [
            None,
            Some(queued_revision()),
            Some(Revision::default()),
            None,
            Some(staged_revision())
        ],
        attached(PreloadMode::Crossfade(Speed::default()))
    )]
    #[case::advance_moves_queued_to_primary(
        Deck::advance,
        [Some(queued_revision()), None, None, None, Some(staged_revision())],
        unsignalled_primary()
    )]
    #[case::append_staged_moves_the_staged_track_to_primary(
        append_staged,
        [Some(staged_revision()), Some(queued_revision()), None, None, None],
        None
    )]
    #[case::silence_drops_every_slot(silence, [None, None, None, None, None], None)]
    #[case::clear_staged_drops_only_the_staged_track(
        clear_staged,
        [None, Some(queued_revision()), None, None, None],
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
        let (spectrum, _spectrum_tap) = tap::new_tap();
        let (sender, _heard) = crossbeam_channel::bounded(64);
        let mut deck = Deck::new(spectrum, sender);
        deck.output = Some(detached_output());
        deck
    }

    pub(crate) fn tone(millis: u64) -> impl Source {
        SineWave::new(440.0).take_duration(Duration::from_millis(millis))
    }

    #[test]
    fn a_finished_primary_sink_reports_the_track_finished() {
        let mut deck = deck_with_detached_output();
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(1), Revision::default().next(), wake);
        for _ in source {}
        let revision = control.revision();
        output(&mut deck).control = Some(control);

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
    fn a_gapless_queued_track_keeps_its_signals_until_it_holds_the_primary_slot() {
        let mut deck = deck_with_detached_output();
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(1), Revision::default().next(), wake);
        for _ in source {}
        let revision = control.revision();
        output(&mut deck).queued_control = Some(control);

        assert!(deck.take_signals(revision).is_none());
        assert!(matches!(
            deck.advance(),
            Some(AudioMessage::SignalsTaken { role: SinkRole::Primary, signals })
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
        let (wake, _heard) = crossbeam_channel::bounded(8);
        let (source, mut outgoing) = envelope(tone(100), preloaded, wake.clone());
        outgoing.ramp(Ramp::fade_out(outgoing.frames(Duration::from_millis(10))));
        for _ in source {}
        let revision = outgoing.revision();
        let (_source, primary) = envelope(tone(100), decoded, wake);
        let output = output(&mut deck);
        output.control = Some(outgoing);
        output.retire_sink(Speed::default());
        output.control = Some(primary);

        assert!(matches!(
            deck.take_signals(revision),
            Some(AudioMessage::SignalsTaken { role: SinkRole::Outgoing, signals })
                if signals.contains(Signals::RAMPED)
        ));
    }
}
