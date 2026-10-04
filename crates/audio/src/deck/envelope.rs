use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::Sender;
use kernel::update::{Machine, Unhandled};
use rodio::Source;
use triple_buffer::{Input, Output, triple_buffer};

use crate::{
    deck::{DeckEvent, Revision},
    engine::{
        crossfade::{gain_in, gain_out},
        effect::{AudioMessage, SinkRole},
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Curve {
    EqualPowerIn,
    EqualPowerOut,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Ramp {
    pub(crate) from: f32,
    pub(crate) to: f32,
    pub(crate) curve: Curve,
    pub(crate) frames: u64,
}

impl Ramp {
    pub(crate) fn fade_in(frames: u64) -> Self {
        Self {
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
            frames,
        }
    }

    pub(crate) fn fade_out(frames: u64) -> Self {
        Self {
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
            frames,
        }
    }

    pub(crate) fn hold(level: f32) -> Self {
        Self {
            from: level,
            to: level,
            curve: Curve::EqualPowerIn,
            frames: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Order {
    pub(crate) serial: u32,
    pub(crate) ramp: Option<Ramp>,
    pub(crate) cue: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Signals(u8);

impl Signals {
    pub(crate) const FINISHED: Self = Self(1);
    pub(crate) const CUED: Self = Self(2);
    pub(crate) const RAMPED: Self = Self(4);

    #[must_use]
    pub(crate) fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}

struct Published {
    gain: AtomicU32,
    flags: AtomicU8,
    position: AtomicU64,
}

impl Published {
    fn fresh() -> Self {
        Self {
            gain: AtomicU32::new(1.0f32.to_bits()),
            flags: AtomicU8::new(0),
            position: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frames(u64);

impl Frames {
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

#[derive(Default)]
pub(crate) struct Envelopes {
    pub(crate) primary: Option<EnvelopeControl>,
    pub(crate) queued: Option<EnvelopeControl>,
    pub(crate) incoming: Option<EnvelopeControl>,
    pub(crate) outgoing: Option<EnvelopeControl>,
}

impl Envelopes {
    #[must_use]
    pub(crate) fn holding(&self, revision: Revision) -> Option<&EnvelopeControl> {
        [&self.primary, &self.queued, &self.incoming, &self.outgoing]
            .into_iter()
            .filter_map(Option::as_ref)
            .find(|control| control.revision() == revision)
    }

    #[must_use]
    pub(crate) fn role(&self, revision: Revision) -> Option<SinkRole> {
        [
            (&self.primary, SinkRole::Primary),
            (&self.incoming, SinkRole::Incoming),
            (&self.outgoing, SinkRole::Outgoing),
        ]
        .into_iter()
        .find_map(|(slot, role)| {
            slot.as_ref()
                .filter(|control| control.revision() == revision)
                .map(|_| role)
        })
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
    pub(crate) fn frames(&self, duration: Duration) -> u64 {
        Frames::of(duration, self.rate).0
    }

    fn order(&mut self, edit: impl FnOnce(&mut Order)) {
        self.pending.serial += 1;
        edit(&mut self.pending);
        self.orders.write(self.pending);
    }

    pub(crate) fn ramp(&mut self, ramp: Ramp) {
        self.order(|order| order.ramp = Some(ramp));
    }

    pub(crate) fn cue(&mut self, at: Option<Duration>) {
        self.order(|order| order.cue = at);
    }

    #[must_use]
    pub(crate) fn take_signals(&self) -> Signals {
        Signals(self.published.flags.swap(0, Ordering::Acquire))
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn gain(&self) -> f32 {
        f32::from_bits(self.published.gain.load(Ordering::Relaxed))
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
    elapsed: u64,
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
    frames: u64,
    rate: u32,
    base: Duration,
    channel: u16,
    gain: f32,
    running: Option<Running>,
    cue: Option<u64>,
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
        frames: 0,
        rate,
        base: Duration::ZERO,
        channel: 0,
        gain: 1.0,
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

fn curved_gain(ramp: Ramp, fraction: f32) -> f32 {
    match ramp.curve {
        Curve::EqualPowerIn => ramp.from + (ramp.to - ramp.from) * gain_in(fraction),
        Curve::EqualPowerOut => ramp.to + (ramp.from - ramp.to) * gain_out(fraction),
    }
}

impl<S: Source> Envelope<S> {
    fn raise(&mut self, flag: Signals) {
        let previous = self.published.flags.fetch_or(flag.0, Ordering::Release);
        if previous & flag.0 == 0 {
            self.wake();
        }
    }

    fn wake(&mut self) {
        self.wake = match DeckEvent::Track(self.revision).notify(&self.sender) {
            Ok(()) => Wake::Sent,
            Err(_) => Wake::Pending,
        };
    }

    fn end(&mut self) -> Option<f32> {
        if self.ending == Ending::Playing {
            self.ending = Ending::Ended;
            self.raise(Signals::FINISHED);
        }
        (self.wake == Wake::Pending).then_some(0.0)
    }

    fn reconcile_rate(&mut self) {
        let rate = self.inner.sample_rate();
        if rate != self.rate {
            self.base += Frames(self.frames).duration(self.rate);
            self.frames = 0;
            self.rate = rate;
        }
    }

    fn advance_cue(&mut self) {
        let Some(target) = self.cue else {
            return;
        };
        if self.frames >= target {
            self.raise(Signals::CUED);
            self.cue = None;
        }
    }

    fn advance_ramp(&mut self) {
        let Some(mut running) = self.running else {
            return;
        };
        running.elapsed += 1;
        if running.elapsed >= running.ramp.frames {
            self.gain = running.ramp.to;
            self.running = None;
            self.raise(Signals::RAMPED);
            return;
        }
        let elapsed = Frames(running.elapsed).duration(self.rate);
        let total = Frames(running.ramp.frames).duration(self.rate);
        let fraction = if total.is_zero() {
            1.0
        } else {
            elapsed.as_secs_f32() / total.as_secs_f32()
        };
        self.gain = curved_gain(running.ramp, fraction);
        self.running = Some(running);
    }

    fn publish(&self) {
        self.published
            .gain
            .store(self.gain.to_bits(), Ordering::Relaxed);
        let position = self.base + Frames(self.frames).duration(self.rate);
        let nanos = u64::try_from(position.as_nanos()).unwrap_or(u64::MAX);
        self.published.position.store(nanos, Ordering::Relaxed);
    }

    fn advance_frame(&mut self) {
        self.frames += 1;
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
        if order.serial == self.previous.serial {
            return Err(Unhandled);
        }
        if order.ramp != self.previous.ramp
            && let Some(ramp) = order.ramp
        {
            let from = if self.running.is_some() {
                self.gain
            } else {
                ramp.from
            };
            self.running = Some(Running {
                ramp: Ramp { from, ..ramp },
                elapsed: 0,
            });
        }
        if order.cue != self.previous.cue {
            self.cue = order.cue.map(|at| Frames::of(at, self.rate).0);
        }
        self.previous = order;
        Ok(())
    }
}

impl<S: Source> Iterator for Envelope<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.wake == Wake::Pending {
            self.wake();
        }
        let Some(sample) = self.inner.next() else {
            return self.end();
        };
        let channels = self.inner.channels();
        self.channel += 1;
        if self.channel >= channels {
            self.channel = 0;
            self.advance_frame();
        }
        Some(sample * self.gain)
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
        self.frames = 0;
        self.rate = self.inner.sample_rate();
        self.ending = Ending::Playing;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rodio::{Source, source::SineWave};
    use rstest::rstest;

    use crate::deck::{
        DeckEvent,
        Revision,
        envelope::{Curve, Ramp, Signals, envelope},
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
        control.ramp(Ramp::fade_out(441));
        drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::RAMPED));
        assert!(flags.contains(Signals::FINISHED));
        assert!((control.gain() - 0.0).abs() < 1e-4);
    }

    #[rstest]
    fn an_envelope_with_no_order_stays_at_unity_gain() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(100), Revision::default(), wake);
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert!((control.gain() - 1.0).abs() < 1e-6);
    }

    #[rstest]
    fn a_wake_up_that_does_not_fit_is_delivered_on_the_next_callback() {
        let (wake, heard) = crossbeam_channel::bounded(1);
        assert!(DeckEvent::Track(Revision::default()).notify(&wake).is_ok());
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
    fn a_newer_order_retargets_the_ramp() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (mut source, mut control) = envelope(tone(100), Revision::default(), wake);
        control.ramp(Ramp::fade_out(4410));
        for _ in 0..441 {
            source.next();
        }
        let mid_gain = control.gain();
        assert!(mid_gain < 1.0 && mid_gain > 0.0);
        control.ramp(Ramp {
            from: 1.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
            frames: 441,
        });
        for _ in 0..441 {
            source.next();
        }
        let after_retarget = control.gain();
        assert!(after_retarget >= mid_gain);
        drain(source);
        assert!((control.gain() - 1.0).abs() < 1e-4);
    }
}
