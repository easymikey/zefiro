use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::Sender;
use kernel::{
    domain::revision::Revision,
    update::machine::{Machine, Unhandled},
};
use num_traits::ToPrimitive;
use rodio::Source;
use triple_buffer::{Input, Output, triple_buffer};

use crate::{
    deck::event::DeckEvent,
    engine::{
        crossfade::{equal_power_in, equal_power_out},
        message::{AudioMessage, Signals},
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

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Order {
    pub(crate) ramp_serial: u32,
    pub(crate) ramp: Option<Ramp>,
    pub(crate) fade_start_serial: u32,
    pub(crate) fade_start: Option<Duration>,
}

struct EnvelopeReadout {
    flags: AtomicU8,
    position_bits: AtomicU64,
}

impl EnvelopeReadout {
    fn new() -> Self {
        Self {
            flags: AtomicU8::new(0),
            position_bits: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Frames(u64);

impl Frames {
    const ZERO: Self = Self(0);

    fn advance(&mut self) {
        self.0 += 1;
    }

    fn duration(self, rate: u32) -> Duration {
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

    fn from_duration(duration: Duration, rate: u32) -> Self {
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

    #[must_use]
    pub(crate) fn take_signals(&self) -> Signals {
        Signals(self.envelope_readout.flags.swap(0, Ordering::Acquire))
    }

    #[must_use]
    pub(crate) fn position(&self) -> Duration {
        Duration::from_nanos(
            self.envelope_readout.position_bits.load(Ordering::Relaxed),
        )
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    Sent,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Playing,
    Ended,
}

pub(crate) struct Envelope<S> {
    inner: S,
    output: Output<Order>,
    envelope_readout: Arc<EnvelopeReadout>,
    callback_sender: Sender<AudioMessage>,
    revision: Revision,
    frames: Frames,
    rate: u32,
    offset: Duration,
    channel: u16,
    gain: Gain,
    running: Option<Running>,
    fade_start: Option<Duration>,
    previous: Order,
    wake: Wake,
    ending: Ending,
}

pub(crate) fn envelope<S: Source>(
    inner: S,
    revision: Revision,
    callback_sender: Sender<AudioMessage>,
) -> (Envelope<S>, EnvelopeControl) {
    let rate = inner.sample_rate();
    let envelope_readout = Arc::new(EnvelopeReadout::new());
    let (orders_input, orders_output) = triple_buffer(&Order::default());
    let envelope = Envelope {
        inner,
        output: orders_output,
        envelope_readout: Arc::clone(&envelope_readout),
        callback_sender,
        revision,
        frames: Frames::ZERO,
        rate,
        offset: Duration::ZERO,
        channel: 0,
        gain: Gain::UNITY,
        running: None,
        fade_start: None,
        previous: Order::default(),
        wake: Wake::Sent,
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

impl<S: Source> Envelope<S> {
    fn raise(&mut self, signals: Signals) {
        let previous = self
            .envelope_readout
            .flags
            .fetch_or(signals.0, Ordering::Release);
        if previous & signals.0 == 0 {
            self.wake();
        }
    }

    fn wake(&mut self) {
        self.wake = match DeckEvent::Woke(self.revision).wake(&self.callback_sender) {
            Ok(()) => Wake::Sent,
            Err(_) => Wake::Pending,
        };
    }

    fn retry_wake(&mut self) {
        if self.wake == Wake::Pending {
            self.wake();
        }
    }

    fn end(&mut self) -> Option<f32> {
        if self.ending == Ending::Playing {
            self.ending = Ending::Ended;
            self.raise(Signals::FINISHED);
        }
        if self.wake == Wake::Sent {
            return None;
        }
        self.channel += 1;
        if self.channel >= self.inner.channels() {
            self.channel = 0;
            self.advance_frame();
        }
        (self.wake == Wake::Pending).then_some(0.0)
    }

    fn reconcile_rate(&mut self) {
        let rate = self.inner.sample_rate();
        if rate != self.rate {
            self.offset += self.frames.duration(self.rate);
            self.frames = Frames::ZERO;
            self.rate = rate;
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

    fn publish(&self, position: Duration) {
        let nanos = u64::try_from(position.as_nanos()).unwrap_or(u64::MAX);
        self.envelope_readout
            .position_bits
            .store(nanos, Ordering::Relaxed);
    }

    fn advance_frame(&mut self) {
        self.frames.advance();
        self.retry_wake();
        self.reconcile_rate();
        if self.output.update() {
            let order = *self.output.output_buffer();
            match self.transition(EnvelopeMessage::Order(order)) {
                Ok(()) | Err(Unhandled) => {}
            }
        }
        let position = self.offset + self.frames.duration(self.rate);
        self.advance_fade_start(position);
        self.advance_ramp();
        self.publish(position);
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum EnvelopeMessage {
    Order(Order),
}

impl<S: Source> Machine for Envelope<S> {
    type Message = EnvelopeMessage;
    type Effect = ();

    fn transition(&mut self, message: EnvelopeMessage) -> Result<(), Unhandled> {
        let EnvelopeMessage::Order(order) = message;
        let ramped = order.ramp_serial != self.previous.ramp_serial;
        let fade_start_ordered =
            order.fade_start_serial != self.previous.fade_start_serial;
        if !ramped && !fade_start_ordered {
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
        self.previous = order;
        Ok(())
    }
}

impl<S: Source> Iterator for Envelope<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let Some(sample) = self.inner.next() else {
            return self.end();
        };
        let channels = self.inner.channels();
        self.channel += 1;
        if self.channel >= channels {
            self.channel = 0;
            self.advance_frame();
        }
        Some(sample * self.gain.amplitude())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Envelope<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(position)?;
        self.offset = position;
        self.frames = Frames::ZERO;
        self.rate = self.inner.sample_rate();
        self.ending = Ending::Playing;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::revision::Revision;
    use rodio::{Source, buffer::SamplesBuffer, source::SineWave};
    use rstest::rstest;

    use crate::{
        deck::{
            envelope::{Curve, Frames, Ramp, envelope},
            event::DeckEvent,
        },
        engine::message::Signals,
        gain::Gain,
    };

    fn tone(millis: u64) -> impl Source {
        SineWave::new(440.0).take_duration(Duration::from_millis(millis))
    }

    fn drain<S: Source>(source: S) -> Vec<f32> {
        let mut samples = Vec::new();
        for sample in source {
            samples.push(sample);
        }
        samples
    }

    #[rstest]
    fn an_envelope_raises_finished_when_the_source_ends() {
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(4);
        let (source, control) =
            envelope(tone(100), Revision::default(), callback_sender);
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert_eq!(callback_receiver.len(), 1);
    }

    #[rstest]
    fn an_envelope_raises_fade_start_at_its_fade_start() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, mut control) =
            envelope(tone(100), Revision::default(), callback_sender);
        control.set_fade_start(Some(Duration::from_millis(50)));
        drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::FADE_START));
        assert!(flags.contains(Signals::FINISHED));
    }

    #[rstest]
    fn an_envelope_raises_ramped_and_ends_at_the_target_gain() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, mut control) =
            envelope(tone(100), Revision::default(), callback_sender);
        control.ramp(Ramp::fade_out(Frames(441)));
        let samples = drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::RAMPED));
        assert!(flags.contains(Signals::FINISHED));
        assert!(samples[500..].iter().all(|sample| sample.abs() < 1e-4));
    }

    #[rstest]
    fn an_envelope_with_no_order_stays_at_unity_gain() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, control) =
            envelope(tone(100), Revision::default(), callback_sender);
        let samples = drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert_eq!(samples, drain(tone(100)));
    }

    #[rstest]
    fn a_wake_up_that_does_not_fit_is_delivered_on_the_next_callback() {
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(1);
        assert!(
            DeckEvent::Woke(Revision::default())
                .wake(&callback_sender)
                .is_ok()
        );
        let (mut source, control) =
            envelope(tone(10), Revision::default(), callback_sender);
        assert_eq!(source.by_ref().take(4096).count(), 4096);
        assert!(control.take_signals().contains(Signals::FINISHED));
        assert!(callback_receiver.try_recv().is_ok());
        assert!(callback_receiver.is_empty());
        assert_eq!(source.next(), None);
        assert_eq!(callback_receiver.len(), 1);
        assert_eq!(source.next(), None);
        assert_eq!(callback_receiver.len(), 1);
    }

    #[rstest]
    fn a_deck_wakes_once_per_frame_after_its_source_ends() {
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(1);
        assert!(
            DeckEvent::Woke(Revision::default())
                .wake(&callback_sender)
                .is_ok()
        );
        let samples = SamplesBuffer::new(3, 44_100, vec![0.0; 3]);
        let (mut source, _control) =
            envelope(samples, Revision::default(), callback_sender);
        assert_eq!(source.by_ref().take(3).count(), 3);
        assert_eq!(source.next(), Some(0.0));
        assert!(callback_receiver.try_recv().is_ok());
        assert_eq!(source.next(), Some(0.0));
        assert!(callback_receiver.is_empty());
        assert_eq!(source.next(), None);
        assert_eq!(callback_receiver.len(), 1);
    }

    #[rstest]
    fn a_seek_rebases_the_published_position() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (mut source, control) =
            envelope(tone(200), Revision::default(), callback_sender);
        for _ in 0..100 {
            source.next();
        }
        source.try_seek(Duration::from_millis(150)).unwrap();
        source.next();
        let position = control.position().as_secs_f32();
        assert!((position - 0.150).abs() < 1e-3, "got {position}");
    }

    #[rstest]
    fn a_fade_start_after_a_seek_fires_at_the_track_position() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (mut source, mut control) =
            envelope(tone(1000), Revision::default(), callback_sender);
        control.set_fade_start(Some(Duration::from_millis(500)));
        source.next();
        source.try_seek(Duration::from_millis(400)).unwrap();
        while control.position() < Duration::from_millis(550) {
            source.next();
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
    }

    #[rstest]
    fn a_fade_start_set_again_at_the_same_time_fires_again() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (mut source, mut control) =
            envelope(tone(1000), Revision::default(), callback_sender);
        control.set_fade_start(Some(Duration::from_millis(500)));
        while control.position() < Duration::from_millis(600) {
            source.next();
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
        source.try_seek(Duration::from_millis(400)).unwrap();
        control.set_fade_start(Some(Duration::from_millis(500)));
        source.next();
        while control.position() < Duration::from_millis(600) {
            source.next();
        }
        assert!(control.take_signals().contains(Signals::FADE_START));
    }

    #[rstest]
    fn a_crossfade_cancelled_by_a_hold_raises_no_ramped() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, mut control) =
            envelope(tone(100), Revision::default(), callback_sender);
        control.ramp(Ramp::hold(Gain::UNITY));
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
    }

    #[rstest]
    fn a_newer_order_retargets_the_ramp() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (mut source, mut control) =
            envelope(tone(100), Revision::default(), callback_sender);
        control.ramp(Ramp::fade_out(Frames(4410)));
        let faded: Vec<f32> = source.by_ref().take(441).collect();
        control.ramp(Ramp {
            from: Gain::UNITY,
            to: Gain::UNITY,
            curve: Curve::EqualPowerIn,
            length: Frames(441),
        });
        let retargeted: Vec<f32> = source.by_ref().take(441).collect();
        let rest = drain(source);
        let plain = drain(tone(100));
        let energy =
            |samples: &[f32]| samples.iter().map(|sample| sample * sample).sum::<f32>();
        assert!(energy(&faded) > 0.0 && energy(&faded) < energy(&plain[..441]));
        assert!(energy(&retargeted) > 0.0);
        assert_eq!(rest.len(), plain.len() - 882);
        assert!(
            rest.iter()
                .zip(&plain[882..])
                .all(|(sample, unity)| (sample - unity).abs() < 1e-4)
        );
    }
}
