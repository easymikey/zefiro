use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use kernel::{
    cmd::Playback,
    domain::revision::Revision,
    update::machine::{Machine, Unhandled},
};
use num_traits::ToPrimitive;
use triple_buffer::{Input, Output, triple_buffer};

use crate::{
    deck::varispeed::OutputFormat,
    engine::{
        crossfade::{equal_power_in, equal_power_out},
        message::Signals,
    },
    gain::Gain,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Curve {
    EqualPowerIn,
    EqualPowerOut,
    Hold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ramp {
    pub(crate) from: Gain,
    pub(crate) to: Gain,
    pub(crate) curve: Curve,
    pub(crate) length: Frames,
}

impl Ramp {
    pub(crate) fn fade_in(length: Frames) -> Self {
        Self {
            from: Gain::SILENCE,
            to: Gain::UNITY,
            curve: Curve::EqualPowerIn,
            length,
        }
    }

    pub(crate) fn fade_out(length: Frames) -> Self {
        Self {
            from: Gain::UNITY,
            to: Gain::SILENCE,
            curve: Curve::EqualPowerOut,
            length,
        }
    }

    pub(crate) fn hold(gain: Gain) -> Self {
        Self {
            from: gain,
            to: gain,
            curve: Curve::Hold,
            length: Frames::ZERO,
        }
    }
}

pub(crate) const VOLUME_GLIDE: Frames = Frames(256);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Order {
    pub(crate) ramp_serial: u32,
    pub(crate) ramp: Option<Ramp>,
    pub(crate) fade_start_serial: u32,
    pub(crate) fade_start: Option<Duration>,
    pub(crate) gain: Gain,
}

impl Default for Order {
    fn default() -> Self {
        Self {
            ramp_serial: 0,
            ramp: None,
            fade_start_serial: 0,
            fade_start: None,
            gain: Gain::UNITY,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct EnvelopeReadout {
    pub(crate) flags: AtomicU8,
    position_bits: AtomicU64,
    playback: AtomicU8,
}

impl EnvelopeReadout {
    pub(crate) fn publish(&self, position: Duration) {
        let nanos = u64::try_from(position.as_nanos()).unwrap_or(u64::MAX);
        self.position_bits.store(nanos, Ordering::Relaxed);
    }

    pub(crate) fn position(&self) -> Duration {
        Duration::from_nanos(self.position_bits.load(Ordering::Relaxed))
    }

    pub(crate) fn pace(&self, playback: Playback) {
        self.playback.store(
            match playback {
                Playback::Playing => 0,
                Playback::Paused => 1,
            },
            Ordering::Release,
        );
    }

    pub(crate) fn playback(&self) -> Playback {
        match self.playback.load(Ordering::Acquire) {
            0 => Playback::Playing,
            _ => Playback::Paused,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Frames(pub(crate) u64);

impl Frames {
    pub(crate) const ZERO: Self = Self(0);

    fn advance(&mut self) {
        self.0 += 1;
    }

    pub(crate) fn duration(self, rate: u32) -> Duration {
        if rate == 0 {
            return Duration::ZERO;
        }
        let rate = u64::from(rate);
        Duration::from_secs(self.0 / rate)
            + Duration::from_nanos(self.0 % rate * 1_000_000_000 / rate)
    }

    fn fraction_of(self, total: Frames) -> f32 {
        self.0.to_f32().unwrap_or(0.0) / total.0.to_f32().unwrap_or(1.0)
    }

    pub(crate) fn from_duration(duration: Duration, rate: u32) -> Self {
        let frames = duration.as_nanos() * u128::from(rate) / 1_000_000_000u128;
        Self(u64::try_from(frames).unwrap_or(u64::MAX))
    }
}

pub(crate) struct EnvelopeControl {
    input: Input<Order>,
    envelope_readout: Arc<EnvelopeReadout>,
    revision: Revision,
    pending_order: Order,
    rate: u32,
}

impl EnvelopeControl {
    #[must_use]
    pub(crate) fn frames(&self, duration: Duration) -> Frames {
        Frames::from_duration(duration, self.rate)
    }

    fn order(&mut self, edit: impl FnOnce(&mut Order)) {
        edit(&mut self.pending_order);
        self.input.write(self.pending_order);
    }

    pub(crate) fn ramp(&mut self, ramp: Ramp) {
        self.order(|order| {
            order.ramp = Some(ramp);
            order.ramp_serial += 1;
        });
    }

    pub(crate) fn set_fade_start(&mut self, fade_start: Option<Duration>) {
        self.order(|order| {
            order.fade_start = fade_start;
            order.fade_start_serial += 1;
        });
    }

    pub(crate) fn set_volume(&mut self, gain: Gain) {
        self.order(|order| order.gain = gain);
    }

    #[must_use]
    pub(crate) fn volume(&self) -> Gain {
        self.pending_order.gain
    }

    #[must_use]
    pub(crate) fn take_signals(&self) -> Signals {
        Signals(self.envelope_readout.flags.swap(0, Ordering::Acquire))
    }

    #[must_use]
    pub(crate) fn position(&self) -> Duration {
        self.envelope_readout.position()
    }

    pub(crate) fn pace(&self, playback: Playback) {
        self.envelope_readout.pace(playback);
    }

    pub(crate) fn revision(&self) -> Revision {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Running {
    ramp: Ramp,
    elapsed_frames: Frames,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    from: Gain,
    to: Gain,
    elapsed_frames: Frames,
}

impl Glide {
    fn advance(mut self) -> (Gain, Option<Self>) {
        self.elapsed_frames.advance();
        if self.elapsed_frames >= VOLUME_GLIDE {
            return (self.to, None);
        }
        let fraction = self.elapsed_frames.fraction_of(VOLUME_GLIDE);
        let (from, to) = (self.from.amplitude(), self.to.amplitude());
        (
            Gain::from_amplitude(from + (to - from) * fraction),
            Some(self),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Playing,
    Ended,
}

pub(crate) struct Envelope {
    output: Output<Order>,
    envelope_readout: Arc<EnvelopeReadout>,
    frames: Frames,
    channel: u16,
    channels: u16,
    gain: Gain,
    running: Option<Running>,
    current_gain: Gain,
    glide: Option<Glide>,
    fade_start: Option<Duration>,
    previous: Order,
    ending: Ending,
}

impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelope")
            .field("frames", &self.frames)
            .field("gain", &self.gain)
            .finish_non_exhaustive()
    }
}

pub(crate) fn envelope(
    format: OutputFormat,
    revision: Revision,
    envelope_readout: Arc<EnvelopeReadout>,
) -> (Envelope, EnvelopeControl) {
    let OutputFormat { channels, rate } = format;
    let (orders_input, orders_output) = triple_buffer(&Order::default());
    let envelope = Envelope {
        output: orders_output,
        envelope_readout: Arc::clone(&envelope_readout),
        frames: Frames::ZERO,
        channel: 0,
        channels,
        gain: Gain::UNITY,
        running: None,
        current_gain: Gain::UNITY,
        glide: None,
        fade_start: None,
        previous: Order::default(),
        ending: Ending::Playing,
    };
    let control = EnvelopeControl {
        input: orders_input,
        envelope_readout,
        revision,
        pending_order: Order::default(),
        rate,
    };
    (envelope, control)
}

fn curved_gain(ramp: Ramp, fraction: f32) -> Gain {
    let (from, to) = (ramp.from.amplitude(), ramp.to.amplitude());
    Gain::from_amplitude(match ramp.curve {
        Curve::EqualPowerIn => from + (to - from) * equal_power_in(fraction),
        Curve::EqualPowerOut => to + (from - to) * equal_power_out(fraction),
        Curve::Hold => to,
    })
}

impl Envelope {
    pub(crate) fn read(&mut self, out: &mut [f32]) {
        for sample in out.iter_mut() {
            self.channel += 1;
            if self.channel >= self.channels {
                self.channel = 0;
                self.advance_frame();
            }
            *sample = *sample * self.gain.amplitude() * self.current_gain.amplitude();
        }
    }

    pub(crate) fn seek(&mut self) {
        self.frames = Frames::ZERO;
        self.ending = Ending::Playing;
    }

    fn raise(&self, signals: Signals) {
        self.envelope_readout
            .flags
            .fetch_or(signals.0, Ordering::Release);
    }

    pub(crate) fn end(&mut self) {
        if self.ending == Ending::Playing {
            self.ending = Ending::Ended;
            self.raise(Signals::FINISHED);
        }
    }

    fn advance_fade_start(&mut self, position: Duration) {
        let Some(fade_start) = self.fade_start else {
            return;
        };
        if position >= fade_start {
            self.raise(Signals::FADE_START);
            self.fade_start = None;
        }
    }

    fn advance_ramp(&mut self) {
        let Some(mut running) = self.running else {
            return;
        };
        running.elapsed_frames.advance();
        if running.elapsed_frames >= running.ramp.length {
            self.gain = running.ramp.to;
            self.running = None;
            if running.ramp.curve != Curve::Hold {
                self.raise(Signals::RAMPED);
            }
            return;
        }
        let fraction = running.elapsed_frames.fraction_of(running.ramp.length);
        self.gain = curved_gain(running.ramp, fraction);
        self.running = Some(running);
    }

    fn advance_frame(&mut self) {
        if self.output.update() {
            let order = *self.output.output_buffer();
            match self.transition(EnvelopeMessage::Order(order)) {
                Ok(()) | Err(Unhandled) => {}
            }
        }
        self.frames.advance();
        self.advance_fade_start(self.envelope_readout.position());
        self.advance_ramp();
        if let Some(glide) = self.glide {
            (self.current_gain, self.glide) = glide.advance();
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum EnvelopeMessage {
    Order(Order),
}

impl Machine for Envelope {
    type Message = EnvelopeMessage;
    type Effect = ();

    fn transition(&mut self, message: EnvelopeMessage) -> Result<(), Unhandled> {
        let EnvelopeMessage::Order(order) = message;
        let ramped = order.ramp_serial != self.previous.ramp_serial;
        let fade_start_ordered =
            order.fade_start_serial != self.previous.fade_start_serial;
        let glide = (order.gain != self.previous.gain).then_some(Glide {
            from: self.current_gain,
            to: order.gain,
            elapsed_frames: Frames::ZERO,
        });
        if !ramped && !fade_start_ordered && glide.is_none() {
            return Err(Unhandled);
        }
        if ramped && let Some(ramp) = order.ramp {
            let from = if self.running.is_some() {
                self.gain
            } else {
                ramp.from
            };
            self.running = Some(Running {
                ramp: Ramp { from, ..ramp },
                elapsed_frames: Frames::ZERO,
            });
        }
        if fade_start_ordered {
            self.fade_start = order.fade_start;
        }
        if let Some(glide) = glide {
            if self.frames == Frames::ZERO {
                self.current_gain = glide.to;
                self.glide = None;
            } else {
                self.glide = Some(glide);
            }
        }
        self.previous = order;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        cmd::Playback,
        domain::revision::Revision,
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        deck::{
            envelope::{
                Curve,
                Envelope,
                EnvelopeMessage,
                Frames,
                Order,
                Ramp,
                VOLUME_GLIDE,
            },
            feed::FeedSource,
            source::tests::decoded,
            tests::{played, pulled},
        },
        engine::message::Signals,
        gain::Gain,
    };

    fn drain(mut source: FeedSource, mut envelope: Envelope) -> Vec<f32> {
        let mut samples = Vec::new();
        loop {
            let block = pulled(&mut source, &mut envelope, 64);
            samples.extend_from_slice(&block);
            if block.len() < 64 {
                return samples;
            }
        }
    }

    #[rstest]
    fn an_envelope_raises_finished_when_the_source_ends() {
        let (_file, source, envelope, control, _feed) =
            played(100, Revision::default());
        drain(source, envelope);
        assert_eq!(control.take_signals(), Signals::FINISHED);
    }

    #[rstest]
    fn an_envelope_raises_fade_start_at_its_fade_start() {
        let (_file, source, envelope, mut control, _feed) =
            played(100, Revision::default());
        control.set_fade_start(Some(Duration::from_millis(50)));
        drain(source, envelope);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::FADE_START));
        assert!(flags.contains(Signals::FINISHED));
    }

    #[rstest]
    fn an_envelope_raises_ramped_and_ends_at_the_target_gain() {
        let (_file, source, envelope, mut control, _feed) =
            played(100, Revision::default());
        control.ramp(Ramp::fade_out(Frames(441)));
        let samples = drain(source, envelope);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::RAMPED));
        assert!(flags.contains(Signals::FINISHED));
        assert!(samples[500..].iter().all(|sample| sample.abs() < 1e-4));
    }

    #[rstest]
    fn an_envelope_with_no_order_stays_at_unity_gain() {
        let (file, source, envelope, control, _feed) = played(100, Revision::default());
        let samples = drain(source, envelope);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert_eq!(samples, decoded(&file));
    }

    #[rstest]
    fn a_seek_rebases_the_published_position() {
        let (_file, mut source, mut envelope, control, mut feed) =
            played(200, Revision::default());
        assert_eq!(pulled(&mut source, &mut envelope, 100).len(), 100);
        source.seek(Duration::from_millis(150));
        envelope.seek();
        feed.prime();
        assert_eq!(pulled(&mut source, &mut envelope, 1).len(), 1);
        let position = control.position().as_secs_f32();
        assert!((position - 0.150).abs() < 1e-3, "got {position}");
    }

    #[rstest]
    fn a_seek_publishes_the_target_before_any_sample_is_pulled() {
        let (_file, mut source, mut envelope, control, _feed) =
            played(3000, Revision::default());
        source.seek(Duration::from_secs(2));
        envelope.seek();
        assert_eq!(control.position(), Duration::from_secs(2));
    }

    #[rstest]
    fn a_paused_seek_reports_the_target_after_play() {
        let (file, mut source, mut envelope, control, mut feed) =
            played(5_000, Revision::default());
        let expected = decoded(&file);
        assert_eq!(pulled(&mut source, &mut envelope, 100).len(), 100);
        control.pace(Playback::Paused);
        source.seek(Duration::from_secs(2));
        envelope.seek();
        assert_eq!(pulled(&mut source, &mut envelope, 1), [0.0]);
        assert_eq!(control.position(), Duration::from_secs(2));
        feed.prime();
        let landed = std::iter::repeat_with(|| pulled(&mut source, &mut envelope, 1))
            .find(|sample| sample != &[0.0]);
        assert_eq!(landed, expected.get(16_000).map(|sample| vec![*sample]));
        assert_eq!(
            control.position(),
            Duration::from_secs(2) + Duration::from_micros(125)
        );
    }

    #[rstest]
    fn a_seek_keeps_playing_the_old_chunks_until_the_target_arrives() {
        let (file, mut source, mut envelope, _control, mut feed) =
            played(5_000, Revision::default());
        let expected = decoded(&file);
        assert_eq!(pulled(&mut source, &mut envelope, 100).len(), 100);
        source.seek(Duration::from_secs(1));
        envelope.seek();
        assert_eq!(pulled(&mut source, &mut envelope, 100), expected[100..200]);
        feed.prime();
        assert_eq!(
            pulled(&mut source, &mut envelope, 1).first(),
            expected.get(8_000)
        );
    }

    #[rstest]
    fn an_underrun_does_not_advance_the_position() {
        let (_file, mut source, mut envelope, control, _feed) =
            played(5_000, Revision::default());
        let (primed, underrun) = (3_000, 256);
        assert_eq!(
            pulled(&mut source, &mut envelope, primed + underrun).len(),
            primed + underrun
        );
        assert_eq!(control.position(), Duration::from_millis(375));
    }

    #[rstest]
    fn a_seek_reports_the_frame_it_landed_on() {
        let (_file, mut source, mut envelope, control, mut feed) =
            played(1_000, Revision::default());
        source.seek(Duration::from_secs(5));
        envelope.seek();
        feed.prime();
        assert_eq!(pulled(&mut source, &mut envelope, 1), []);
        assert_eq!(control.position(), Duration::from_secs(1));
    }

    #[rstest]
    fn a_fade_start_after_a_seek_fires_at_the_track_position() {
        let (_file, mut source, mut envelope, mut control, mut feed) =
            played(1000, Revision::default());
        control.set_fade_start(Some(Duration::from_millis(500)));
        assert_eq!(pulled(&mut source, &mut envelope, 1).len(), 1);
        source.seek(Duration::from_millis(400));
        envelope.seek();
        while control.position() < Duration::from_millis(550) {
            feed.prime();
            assert_eq!(pulled(&mut source, &mut envelope, 1).len(), 1);
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
    }

    #[rstest]
    fn a_fade_start_set_again_at_the_same_time_fires_again() {
        let (_file, mut source, mut envelope, mut control, mut feed) =
            played(1000, Revision::default());
        control.set_fade_start(Some(Duration::from_millis(500)));
        while control.position() < Duration::from_millis(600) {
            feed.prime();
            assert_eq!(pulled(&mut source, &mut envelope, 1).len(), 1);
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
        source.seek(Duration::from_millis(400));
        envelope.seek();
        control.set_fade_start(Some(Duration::from_millis(500)));
        while control.position() < Duration::from_millis(600) {
            feed.prime();
            assert_eq!(pulled(&mut source, &mut envelope, 1).len(), 1);
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
    }

    #[rstest]
    fn a_seek_back_before_the_fade_start_keeps_the_target_while_old_chunks_play() {
        let (_file, mut source, mut envelope, mut control, mut feed) =
            played(1000, Revision::default());
        control.set_fade_start(Some(Duration::from_millis(500)));
        while control.position() < Duration::from_millis(600) {
            feed.prime();
            assert_eq!(pulled(&mut source, &mut envelope, 1).len(), 1);
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
        source.seek(Duration::from_millis(400));
        envelope.seek();
        control.set_fade_start(Some(Duration::from_millis(500)));
        let kept = pulled(&mut source, &mut envelope, 100);
        assert!(kept.iter().any(|sample| *sample != 0.0));
        assert_eq!(control.position(), Duration::from_millis(400));
        assert!(!control.take_signals().contains(Signals::FADE_START));
    }

    #[rstest]
    fn a_crossfade_cancelled_by_a_hold_raises_no_ramped() {
        let (_file, source, envelope, mut control, _feed) =
            played(100, Revision::default());
        control.ramp(Ramp::hold(Gain::UNITY));
        drain(source, envelope);
        assert_eq!(control.take_signals(), Signals::FINISHED);
    }

    #[rstest]
    fn a_newer_order_retargets_the_ramp() {
        let (file, mut source, mut envelope, mut control, _feed) =
            played(100, Revision::default());
        control.ramp(Ramp::fade_out(Frames(800)));
        let faded = pulled(&mut source, &mut envelope, 80);
        control.ramp(Ramp {
            from: Gain::UNITY,
            to: Gain::UNITY,
            curve: Curve::EqualPowerIn,
            length: Frames(80),
        });
        let retargeted = pulled(&mut source, &mut envelope, 80);
        let rest = drain(source, envelope);
        let plain = decoded(&file);
        let energy =
            |samples: &[f32]| samples.iter().map(|sample| sample * sample).sum::<f32>();
        assert!(energy(&faded) > 0.0 && energy(&faded) < energy(&plain[..80]));
        assert!(energy(&retargeted) > 0.0);
        assert_eq!(rest.len(), plain.len() - 160);
        assert!(
            rest.iter()
                .zip(&plain[160..])
                .all(|(sample, unity)| (sample - unity).abs() < 1e-4)
        );
    }

    #[rstest]
    fn a_volume_order_glides_to_its_amplitude_over_volume_glide_frames() {
        let (file, mut source, mut envelope, mut control, _feed) =
            played(100, Revision::default());
        let expected = decoded(&file);
        assert_eq!(pulled(&mut source, &mut envelope, 16).len(), 16);
        control.set_volume(Gain::from_amplitude(0.5));
        let glided: Vec<f32> = pulled(&mut source, &mut envelope, 512)
            .iter()
            .zip(&expected[16..])
            .map(|(sample, unity)| sample / unity)
            .collect();
        let Frames(glide) = VOLUME_GLIDE;
        let glide = usize::try_from(glide).unwrap();
        assert_eq!(glided.len(), 512);
        assert!(glided[0] < 1.0 && glided[0] > 0.5);
        assert!(glided[..glide].windows(2).all(|pair| pair[1] < pair[0]));
        assert!(
            glided[glide - 1..]
                .iter()
                .all(|gain| (gain - 0.5).abs() < 1e-6)
        );
    }

    #[rstest]
    fn a_volume_ordered_before_the_first_frame_applies_at_once() {
        let (file, source, envelope, mut control, _feed) =
            played(100, Revision::default());
        control.set_volume(Gain::from_amplitude(0.25));
        let samples = drain(source, envelope);
        let expected = decoded(&file);
        assert_eq!(samples.len(), expected.len());
        assert!(
            samples
                .iter()
                .zip(&expected)
                .all(|(sample, unity)| (sample - unity * 0.25).abs() < 1e-6)
        );
    }

    #[rstest]
    fn the_same_volume_twice_is_unhandled() {
        let (_file, _source, mut envelope, _control, _feed) =
            played(100, Revision::default());
        let order = Order {
            gain: Gain::from_amplitude(0.5),
            ..Order::default()
        };
        assert!(envelope.transition(EnvelopeMessage::Order(order)).is_ok());
        assert!(matches!(
            envelope.transition(EnvelopeMessage::Order(order)),
            Err(Unhandled)
        ));
    }
}
