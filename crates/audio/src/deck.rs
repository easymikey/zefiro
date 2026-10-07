pub(crate) mod envelope;
pub(crate) mod event;
pub(crate) mod feed;
pub mod job;
pub(crate) mod mixer;
pub(crate) mod output;
pub(crate) mod source;
pub(crate) mod varispeed;

use std::time::Duration;

use crossbeam_channel::{SendError, Sender};
use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, revision::Revision, speed::Speed},
    message::AudioError,
};

use crate::{
    deck::{
        envelope::{EnvelopeControl, Ramp},
        feed::{FeedCmd, feed_channel, play},
        mixer::{MixerOrder, Voice},
        output::{Fader, Output},
        source::{DecodedTrack, PreloadMode},
    },
    device::{OpenedOutput, Opening, OutputLoss, open_output},
    engine::message::{AudioMessage, DeviceOpened, EngineMessage, SinkRole},
    error::DeviceError,
    gain::Gain,
    tap::SpectrumBuffers,
};

pub(crate) struct Deck {
    stream: Option<cpal::Stream>,
    pub(crate) output: Option<Output>,
    staged_track: Option<DecodedTrack>,
    callback_sender: Sender<AudioMessage>,
    feed_sender: Sender<FeedCmd>,
    spectrum_buffers: SpectrumBuffers,
    output_loss: OutputLoss,
}

impl Deck {
    pub(crate) fn new(
        spectrum_buffers: SpectrumBuffers,
        callback_sender: Sender<AudioMessage>,
        feed_sender: Sender<FeedCmd>,
    ) -> Self {
        Self {
            stream: None,
            output: None,
            staged_track: None,
            callback_sender,
            feed_sender,
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
        let revision = decoded_track.revision;
        let path = decoded_track.decoder.path.to_path_buf();
        let speed = match preload_mode {
            PreloadMode::Gapless => Speed::default(),
            PreloadMode::Crossfade(speed) => speed,
        };
        let (source, feed) = feed_channel(
            decoded_track,
            output.format.channels,
            self.callback_sender.clone(),
        );
        let (envelope, mut control) = play(&source, feed, &self.feed_sender);
        let voice = match output.varispeed(source.sample_rate(), speed) {
            Ok(varispeed) => Box::new(Voice::new(source, envelope, varispeed)),
            Err(error) => {
                return Some(
                    EngineMessage::Error(AudioError::Preload { path, error }).into(),
                );
            }
        };
        match preload_mode {
            PreloadMode::Gapless => {
                if let Some(current_gain) =
                    output.current_control.as_ref().map(EnvelopeControl::volume)
                {
                    control.set_volume(current_gain);
                }
                output.mixer_control.order(MixerOrder::Queue(voice));
                output.incoming_control = Some(control);
            }
            PreloadMode::Crossfade(_) => {
                control.set_volume(Gain::SILENCE);
                output.mixer_control.order(MixerOrder::Attach {
                    role: SinkRole::Incoming,
                    voice,
                });
                output.incoming_fader = Some(Fader { control });
            }
        }
        let (_, playback) = output.position();
        self.pace(playback);
        Some(
            EngineMessage::Attached {
                revision,
                preload_mode,
                duration,
            }
            .into(),
        )
    }

    pub(crate) fn pace(&self, playback: Playback) {
        match self.feed_sender.send(FeedCmd::Pace(playback)) {
            Ok(()) | Err(SendError(_)) => {}
        }
    }

    pub(crate) fn transport(&mut self, playback: Playback) {
        if let Some(output) = self.output.as_mut() {
            output.current_playback = playback;
            output.mixer_control.order(MixerOrder::Transport(playback));
        }
        self.pace(playback);
    }

    pub(crate) fn set_speed(&mut self, speed: Speed) {
        if let Some(output) = self.output.as_mut() {
            output.mixer_control.order(MixerOrder::Speed(speed));
        }
    }

    pub(crate) fn seek(&mut self, target: Duration) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let (_, playback) = output.position();
        if let Some(control) = &output.current_control {
            control.pace(playback);
        }
        output.mixer_control.order(MixerOrder::Seek(target));
        self.pace(playback);
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
        self.output.as_ref().map(|output| output.position().0)
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
        let OpenedOutput {
            stream,
            format,
            mixer_control,
            retired_voices,
            device_name,
        } = open_output(
            &device,
            &Opening {
                speed,
                spectrum_buffers: &self.spectrum_buffers,
                callback_sender: &self.callback_sender,
                output_loss: &self.output_loss,
            },
        )?;
        self.drop_preload();
        let (position, playback) = self
            .output
            .as_ref()
            .map_or((Duration::ZERO, Playback::Playing), Output::position);
        drop(self.output.take());
        self.output = Some(Output::new(mixer_control, retired_voices, format));
        self.stream = Some(stream);
        Ok(DeviceOpened {
            device,
            device_name,
            position,
            playback,
        })
    }

    pub(crate) fn drop_preload(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.incoming_fader = None;
            output
                .mixer_control
                .order(MixerOrder::Drop(SinkRole::Incoming));
        }
    }

    pub(crate) fn clear_staged(&mut self) {
        self.staged_track = None;
    }

    pub(crate) fn append_staged(&mut self) -> Option<AudioMessage> {
        let output = self.output.as_mut()?;
        let staged_track = self.staged_track.take()?;
        let path = staged_track.decoder.path.to_path_buf();
        let (source, feed) = feed_channel(
            staged_track,
            output.format.channels,
            self.callback_sender.clone(),
        );
        let (envelope, control) = play(&source, feed, &self.feed_sender);
        let voice = match output.varispeed(source.sample_rate(), Speed::default()) {
            Ok(varispeed) => Box::new(Voice::new(source, envelope, varispeed)),
            Err(error) => {
                return Some(
                    EngineMessage::Error(AudioError::Decode { path, error }).into(),
                );
            }
        };
        output.mixer_control.order(MixerOrder::Attach {
            role: SinkRole::Current,
            voice,
        });
        output.current_control = Some(control);
        let (_, playback) = output.position();
        self.pace(playback);
        None
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

    pub(crate) fn set_current_gain(&mut self, gain: Gain) {
        if let Some(control) = self
            .output
            .as_mut()
            .and_then(|output| output.current_control.as_mut())
        {
            control.set_volume(gain);
        }
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
        if let Some(incoming_fader) = output.incoming_fader.as_mut() {
            incoming_fader.control.set_volume(incoming);
            output.mixer_control.order(MixerOrder::RolePlayback {
                role: SinkRole::Incoming,
                playback: Playback::Playing,
            });
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
            output.mixer_control.order(MixerOrder::RolePlayback {
                role: SinkRole::Incoming,
                playback: Playback::Paused,
            });
        }
    }

    pub(crate) fn drop_outgoing(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.outgoing_fader = None;
            output
                .mixer_control
                .order(MixerOrder::Drop(SinkRole::Outgoing));
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
        if let Some(control) = output.current_control.as_mut() {
            control.set_volume(current);
        }
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
    use std::{io::Write, thread, time::Duration};

    use kernel::domain::{device::OutputDevice, revision::Revision, speed::Speed};
    use rstest::rstest;
    use tempfile::NamedTempFile;

    use crate::{
        deck::{
            Deck,
            envelope::{Envelope, EnvelopeControl, Ramp},
            feed::{Feed, FeedCmd, FeedSource, feed_channel, play},
            output::{Output, tests::detached_output},
            source::{DecodedTrack, PreloadMode, decode, tests::ramp_file},
        },
        engine::{
            message::{AudioMessage, EngineMessage, Signals, SinkRole},
            tests::assert_same,
        },
        gain::Gain,
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
        played(1, revision).3
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
        deck.append_staged()
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
        let (feed_sender, _feed_receiver) = crossbeam_channel::bounded(4);
        let mut deck = Deck::new(spectrum_buffers, callback_sender, feed_sender);
        deck.output = Some(detached_output());
        deck
    }

    pub(crate) fn played(
        millis: u64,
        revision: Revision,
    ) -> (
        NamedTempFile,
        FeedSource,
        Envelope,
        EnvelopeControl,
        Box<Feed>,
    ) {
        let file = ramp_file(1, usize::try_from(millis * 8).unwrap());
        let (feed_sender, feed_receiver) = crossbeam_channel::bounded(1);
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let decoded_track = DecodedTrack {
            revision,
            decoder: decode(file.path()).unwrap(),
        };
        let (source, unserved) = feed_channel(decoded_track, 1, callback_sender);
        let (envelope, control) = play(&source, unserved, &feed_sender);
        let Ok(FeedCmd::Serve(feed)) = feed_receiver.try_recv() else {
            panic!("play sends its feed to the feeder");
        };
        (file, source, envelope, control, feed)
    }

    pub(crate) fn pulled(
        source: &mut FeedSource,
        envelope: &mut Envelope,
        samples: usize,
    ) -> Vec<f32> {
        let mut out = vec![0.0; samples];
        let read = source.read(&mut out);
        out.truncate(read);
        envelope.read(&mut out);
        if read < samples {
            envelope.end();
        }
        out
    }

    #[test]
    #[ignore = "hardware: needs a real audio device; run with --include-ignored"]
    fn a_reopened_deck_keeps_the_spectrum_moving() {
        let (spectrum_buffers, spectrum_tap) = tap::spectrum_channel();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(64);
        let (feed_sender, _feed_receiver) = crossbeam_channel::unbounded();
        let mut deck = Deck::new(spectrum_buffers, callback_sender, feed_sender);
        for _ in 0..2 {
            assert!(
                deck.open(OutputDevice::SystemDefault, Speed::default())
                    .is_ok()
            );
        }

        let mut window = [1.0_f32; tap::WINDOW];
        for _ in 0..2 {
            thread::sleep(Duration::from_millis(300));
            assert!(spectrum_tap.windowed(&[1.0; tap::WINDOW], &mut window));
        }
        assert!(window.iter().all(|&sample| sample == 0.0));
    }

    #[test]
    fn a_finished_current_sink_reports_the_track_finished() {
        let mut deck = deck_with_detached_output();
        let (_file, mut source, mut envelope, control, _feed) =
            played(1, Revision::default().next());
        assert_eq!(pulled(&mut source, &mut envelope, 16).len(), 8);
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
        let (_file, mut source, mut envelope, control, _feed) =
            played(1, Revision::default().next());
        assert_eq!(pulled(&mut source, &mut envelope, 16).len(), 8);
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
        let (_file, mut source, mut envelope, mut outgoing, _feed) =
            played(100, preloaded);
        outgoing.ramp(Ramp::fade_out(outgoing.frames(Duration::from_millis(10))));
        assert_eq!(pulled(&mut source, &mut envelope, 1_024).len(), 800);
        let revision = outgoing.revision();
        let (_current_file, _source, _envelope, current, _current_feed) =
            played(100, decoded);
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

    #[test]
    fn a_gapless_attach_copies_the_current_gain_to_the_queued_control() {
        let mut deck = loaded_deck();
        deck.append_staged();
        deck.set_current_gain(Gain::from_amplitude(0.5));
        assert!(attach_gapless(&mut deck).is_some());
        assert_eq!(
            output(&mut deck)
                .incoming_control
                .as_ref()
                .map(EnvelopeControl::volume),
            Some(Gain::from_amplitude(0.5))
        );
    }
}
