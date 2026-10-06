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
use rodio::Source;
use triple_buffer::{Input, Output, triple_buffer};

use crate::{
    deck::event::DeckEvent,
    engine::{
        crossfade::{gain_in, gain_out},
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

    pub(crate) fn hold(level: Gain) -> Self {
        Self {
            from: level,
            to: level,
            curve: Curve::Hold,
            length: Frames::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Order {
    pub(crate) ramp_serial: u32,
    pub(crate) ramp: Option<Ramp>,
    pub(crate) cue_serial: u32,
    pub(crate) cue: Option<Duration>,
}

struct Published {
    flags: AtomicU8,
    position: AtomicU64,
}

impl Published {
    fn fresh() -> Self {
        Self {
            flags: AtomicU8::new(0),
            position: AtomicU64::new(0),
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
        let nanos = u128::from(self.0) * 1_000_000_000u128 / u128::from(rate);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }

    fn of(duration: Duration, rate: u32) -> Self {
        let frames = duration.as_nanos() * u128::from(rate) / 1_000_000_000u128;
        Self(u64::try_from(frames).unwrap_or(u64::MAX))
    }
}

pub(crate) struct EnvelopeControl {
    orders: Input<Order>,
    published: Arc<Published>,
    revision: Revision,
    pending: Order,
    rate: u32,
}

impl EnvelopeControl {
    #[must_use]
    pub(crate) fn frames(&self, duration: Duration) -> Frames {
        Frames::of(duration, self.rate)
    }

    fn order(&mut self, edit: impl FnOnce(&mut Order)) {
        edit(&mut self.pending);
        self.orders.write(self.pending);
    }

    pub(crate) fn ramp(&mut self, ramp: Ramp) {
        self.order(|order| {
            order.ramp = Some(ramp);
            order.ramp_serial += 1;
        });
    }

    pub(crate) fn cue(&mut self, at: Option<Duration>) {
        self.order(|order| {
            order.cue = at;
            order.cue_serial += 1;
        });
    }

    #[must_use]
    pub(crate) fn take_signals(&self) -> Signals {
        Signals(self.published.flags.swap(0, Ordering::Acquire))
    }

    #[must_use]
    pub(crate) fn position(&self) -> Duration {
        Duration::from_nanos(self.published.position.load(Ordering::Relaxed))
    }

    pub(crate) fn revision(&self) -> Revision {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Running {
    ramp: Ramp,
    elapsed: Frames,
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
    orders: Output<Order>,
    published: Arc<Published>,
    sender: Sender<AudioMessage>,
    revision: Revision,
    frames: Frames,
    rate: u32,
    base: Duration,
    channel: u16,
    gain: Gain,
    running: Option<Running>,
    cue: Option<Duration>,
    previous: Order,
    wake: Wake,
    ending: Ending,
}

pub(crate) fn envelope<S: Source>(
    inner: S,
    revision: Revision,
    sender: Sender<AudioMessage>,
) -> (Envelope<S>, EnvelopeControl) {
    let rate = inner.sample_rate();
    let published = Arc::new(Published::fresh());
    let (orders_input, orders_output) = triple_buffer(&Order::default());
    let envelope = Envelope {
        inner,
        orders: orders_output,
        published: Arc::clone(&published),
        sender,
        revision,
        frames: Frames::ZERO,
        rate,
        base: Duration::ZERO,
        channel: 0,
        gain: Gain::UNITY,
        running: None,
        cue: None,
        previous: Order::default(),
        wake: Wake::Sent,
        ending: Ending::Playing,
    };
    let control = EnvelopeControl {
        orders: orders_input,
        published,
        revision,
        pending: Order::default(),
        rate,
    };
    (envelope, control)
}

fn curved_gain(ramp: Ramp, fraction: f32) -> Gain {
    let (from, to) = (ramp.from.amplitude(), ramp.to.amplitude());
    Gain::from_amplitude(match ramp.curve {
        Curve::EqualPowerIn => from + (to - from) * gain_in(fraction),
        Curve::EqualPowerOut => to + (from - to) * gain_out(fraction),
        Curve::Hold => to,
    })
}

impl<S: Source> Envelope<S> {
    fn raise(&mut self, flag: Signals) {
        let previous = self.published.flags.fetch_or(flag.0, Ordering::Release);
        if previous & flag.0 == 0 {
            self.wake();
        }
    }

    fn wake(&mut self) {
        self.wake = match DeckEvent::Woke(self.revision).wake(&self.sender) {
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
            self.base += self.frames.duration(self.rate);
            self.frames = Frames::ZERO;
            self.rate = rate;
        }
    }

    fn advance_cue(&mut self) {
        let Some(target) = self.cue else {
            return;
        };
        if self.base + self.frames.duration(self.rate) >= target {
            self.raise(Signals::CUED);
            self.cue = None;
        }
    }

    fn advance_ramp(&mut self) {
        let Some(mut running) = self.running else {
            return;
        };
        running.elapsed.advance();
        if running.elapsed >= running.ramp.length {
            self.gain = running.ramp.to;
            self.running = None;
            if running.ramp.curve != Curve::Hold {
                self.raise(Signals::RAMPED);
            }
            return;
        }
        let elapsed = running.elapsed.duration(self.rate);
        let total = running.ramp.length.duration(self.rate);
        let fraction = if total.is_zero() {
            1.0
        } else {
            elapsed.as_secs_f32() / total.as_secs_f32()
        };
        self.gain = curved_gain(running.ramp, fraction);
        self.running = Some(running);
    }

    fn publish(&self) {
        let position = self.base + self.frames.duration(self.rate);
        let nanos = u64::try_from(position.as_nanos()).unwrap_or(u64::MAX);
        self.published.position.store(nanos, Ordering::Relaxed);
    }

    fn advance_frame(&mut self) {
        self.frames.advance();
        self.retry_wake();
        self.reconcile_rate();
        if self.orders.update() {
            let order = *self.orders.output_buffer();
            match self.transition(EnvelopeMessage::Order(order)) {
                Ok(()) | Err(Unhandled) => {}
            }
        }
        self.advance_cue();
        self.advance_ramp();
        self.publish();
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
        let cued = order.cue_serial != self.previous.cue_serial;
        if !ramped && !cued {
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
                elapsed: Frames::ZERO,
            });
        }
        if cued {
            self.cue = order.cue;
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
        self.base = position;
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
        let (wake, heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(100), Revision::default(), wake);
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert_eq!(heard.len(), 1);
    }

    #[rstest]
    fn an_envelope_raises_cued_at_its_cue_point() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, mut control) = envelope(tone(100), Revision::default(), wake);
        control.cue(Some(Duration::from_millis(50)));
        drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::CUED));
        assert!(flags.contains(Signals::FINISHED));
    }

    #[rstest]
    fn an_envelope_raises_ramped_and_ends_at_the_target_gain() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, mut control) = envelope(tone(100), Revision::default(), wake);
        control.ramp(Ramp::fade_out(Frames(441)));
        let samples = drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::RAMPED));
        assert!(flags.contains(Signals::FINISHED));
        assert!(samples[500..].iter().all(|sample| sample.abs() < 1e-4));
    }

    #[rstest]
    fn an_envelope_with_no_order_stays_at_unity_gain() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(100), Revision::default(), wake);
        let samples = drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert_eq!(samples, drain(tone(100)));
    }

    #[rstest]
    fn a_wake_up_that_does_not_fit_is_delivered_on_the_next_callback() {
        let (wake, heard) = crossbeam_channel::bounded(1);
        assert!(DeckEvent::Woke(Revision::default()).wake(&wake).is_ok());
        let (mut source, control) = envelope(tone(10), Revision::default(), wake);
        assert_eq!(source.by_ref().take(4096).count(), 4096);
        assert!(control.take_signals().contains(Signals::FINISHED));
        assert!(heard.try_recv().is_ok());
        assert!(heard.is_empty());
        assert_eq!(source.next(), None);
        assert_eq!(heard.len(), 1);
        assert_eq!(source.next(), None);
        assert_eq!(heard.len(), 1);
    }

    #[rstest]
    fn a_deck_wakes_once_per_frame_after_its_source_ends() {
        let (wake, heard) = crossbeam_channel::bounded(1);
        assert!(DeckEvent::Woke(Revision::default()).wake(&wake).is_ok());
        let samples = SamplesBuffer::new(3, 44_100, vec![0.0; 3]);
        let (mut source, _control) = envelope(samples, Revision::default(), wake);
        assert_eq!(source.by_ref().take(3).count(), 3);
        assert_eq!(source.next(), Some(0.0));
        assert!(heard.try_recv().is_ok());
        assert_eq!(source.next(), Some(0.0));
        assert!(heard.is_empty());
        assert_eq!(source.next(), None);
        assert_eq!(heard.len(), 1);
    }

    #[rstest]
    fn a_seek_rebases_the_published_position() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (mut source, control) = envelope(tone(200), Revision::default(), wake);
        for _ in 0..100 {
            source.next();
        }
        source.try_seek(Duration::from_millis(150)).unwrap();
        source.next();
        let position = control.position().as_secs_f32();
        assert!((position - 0.150).abs() < 1e-3, "got {position}");
    }

    #[rstest]
    fn a_cue_after_a_seek_fires_at_the_track_position() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (mut source, mut control) = envelope(tone(1000), Revision::default(), wake);
        control.cue(Some(Duration::from_millis(500)));
        source.next();
        source.try_seek(Duration::from_millis(400)).unwrap();
        while control.position() < Duration::from_millis(550) {
            source.next();
        }
        assert!(control.take_signals().contains(Signals::CUED));
    }

    #[rstest]
    fn a_cue_armed_again_at_the_same_time_fires_again() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (mut source, mut control) = envelope(tone(1000), Revision::default(), wake);
        control.cue(Some(Duration::from_millis(500)));
        while control.position() < Duration::from_millis(600) {
            source.next();
        }
        assert!(control.take_signals().contains(Signals::CUED));
        source.try_seek(Duration::from_millis(400)).unwrap();
        control.cue(Some(Duration::from_millis(500)));
        source.next();
        while control.position() < Duration::from_millis(600) {
            source.next();
        }
        assert!(control.take_signals().contains(Signals::CUED));
    }

    #[rstest]
    fn a_crossfade_cancelled_by_a_hold_raises_no_ramped() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, mut control) = envelope(tone(100), Revision::default(), wake);
        control.ramp(Ramp::hold(Gain::UNITY));
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
    }

    #[rstest]
    fn a_newer_order_retargets_the_ramp() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (mut source, mut control) = envelope(tone(100), Revision::default(), wake);
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
                .all(|(heard, unity)| (heard - unity).abs() < 1e-4)
        );
    }
}
