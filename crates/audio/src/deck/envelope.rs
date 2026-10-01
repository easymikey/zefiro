use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::Sender;
use rodio::Source;
use triple_buffer::{Input, Output, triple_buffer};

use crate::{
    deck::{DeckEvent, Ticket},
    engine::crossfade::{gain_in, gain_out},
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

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Order {
    pub(crate) serial: u32,
    pub(crate) ramp: Option<Ramp>,
    pub(crate) cue: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Signals(u8);

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

fn frames_to_duration(frames: u64, rate: u32) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(frames) * 1_000_000_000u128 / u128::from(rate);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn duration_to_frames(duration: Duration, rate: u32) -> u64 {
    let frames = duration.as_nanos() * u128::from(rate) / 1_000_000_000u128;
    u64::try_from(frames).unwrap_or(u64::MAX)
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
    pub(crate) fn holding(&self, ticket: Ticket) -> Option<&EnvelopeControl> {
        [&self.primary, &self.queued, &self.incoming, &self.outgoing]
            .into_iter()
            .filter_map(Option::as_ref)
            .find(|control| control.ticket() == ticket)
    }
}

pub(crate) struct EnvelopeControl {
    orders: Input<Order>,
    published: Arc<Published>,
    ticket: Ticket,
    serial: u32,
    pending: Order,
    rate: u32,
}

impl EnvelopeControl {
    #[must_use]
    pub(crate) fn frames(&self, duration: Duration) -> u64 {
        duration_to_frames(duration, self.rate)
    }

    pub(crate) fn ramp(&mut self, ramp: Ramp) {
        self.serial += 1;
        self.pending.serial = self.serial;
        self.pending.ramp = Some(ramp);
        self.orders.write(self.pending);
    }

    pub(crate) fn cue(&mut self, at: Option<Duration>) {
        self.serial += 1;
        self.pending.serial = self.serial;
        self.pending.cue = at;
        self.orders.write(self.pending);
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

    #[must_use]
    pub(crate) fn ticket(&self) -> Ticket {
        self.ticket
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Running {
    ramp: Ramp,
    elapsed: u64,
}

pub(crate) struct Envelope<S> {
    inner: S,
    orders: Output<Order>,
    published: Arc<Published>,
    wake: Sender<DeckEvent>,
    ticket: Ticket,
    frames: u64,
    rate: u32,
    base: Duration,
    channel: u16,
    gain: f32,
    running: Option<Running>,
    cue: Option<u64>,
    applied: u32,
    previous: Order,
}

pub(crate) fn envelope<S: Source>(
    inner: S,
    ticket: Ticket,
    wake: Sender<DeckEvent>,
) -> (Envelope<S>, EnvelopeControl) {
    let rate = inner.sample_rate();
    let published = Arc::new(Published::fresh());
    let (orders_input, orders_output) = triple_buffer(&Order::default());
    let envelope = Envelope {
        inner,
        orders: orders_output,
        published: Arc::clone(&published),
        wake,
        ticket,
        frames: 0,
        rate,
        base: Duration::ZERO,
        channel: 0,
        gain: 1.0,
        running: None,
        cue: None,
        applied: 0,
        previous: Order::default(),
    };
    let control = EnvelopeControl {
        orders: orders_input,
        published,
        ticket,
        serial: 0,
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
    fn raise(&self, flag: Signals) {
        let previous = self.published.flags.fetch_or(flag.0, Ordering::Release);
        if previous & flag.0 == 0 {
            let _ = self.wake.try_send(DeckEvent::Track(self.ticket));
        }
    }

    fn reconcile_rate(&mut self) {
        let rate = self.inner.sample_rate();
        if rate != self.rate {
            self.base += frames_to_duration(self.frames, self.rate);
            self.frames = 0;
            self.rate = rate;
        }
    }

    fn apply_order(&mut self, order: Order) {
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
            self.cue = order.cue.map(|at| duration_to_frames(at, self.rate));
        }
        self.previous = order;
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
        let elapsed = frames_to_duration(running.elapsed, self.rate);
        let total = frames_to_duration(running.ramp.frames, self.rate);
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
        let position = self.base + frames_to_duration(self.frames, self.rate);
        let nanos = u64::try_from(position.as_nanos()).unwrap_or(u64::MAX);
        self.published.position.store(nanos, Ordering::Relaxed);
    }

    fn advance_frame(&mut self) {
        self.frames += 1;
        self.reconcile_rate();
        if self.orders.update() {
            let order = *self.orders.output_buffer();
            if order.serial != self.applied {
                self.applied = order.serial;
                self.apply_order(order);
            }
        }
        self.advance_cue();
        self.advance_ramp();
        self.publish();
    }
}

impl<S: Source> Iterator for Envelope<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let Some(sample) = self.inner.next() else {
            self.raise(Signals::FINISHED);
            return None;
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rodio::{Source, source::SineWave};
    use rstest::rstest;

    use crate::deck::{
        Ticket,
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
        let (source, control) = envelope(tone(100), Ticket::default(), wake);
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert_eq!(heard.len(), 1);
    }

    #[rstest]
    fn an_envelope_raises_cued_at_its_cue_point() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, mut control) = envelope(tone(100), Ticket::default(), wake);
        control.cue(Some(Duration::from_millis(50)));
        drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::CUED));
        assert!(flags.contains(Signals::FINISHED));
    }

    #[rstest]
    fn an_envelope_raises_ramped_and_ends_at_the_target_gain() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, mut control) = envelope(tone(100), Ticket::default(), wake);
        control.ramp(Ramp {
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
            frames: 441,
        });
        drain(source);
        let flags = control.take_signals();
        assert!(flags.contains(Signals::RAMPED));
        assert!(flags.contains(Signals::FINISHED));
        assert!((control.gain() - 0.0).abs() < 1e-4);
    }

    #[rstest]
    fn an_envelope_with_no_order_stays_at_unity_gain() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(100), Ticket::default(), wake);
        drain(source);
        assert_eq!(control.take_signals(), Signals::FINISHED);
        assert!((control.gain() - 1.0).abs() < 1e-6);
    }

    #[rstest]
    fn a_full_wake_channel_loses_no_flag() {
        let (wake, heard) = crossbeam_channel::bounded(1);
        wake.try_send(crate::deck::DeckEvent::Track(Ticket::default()))
            .unwrap();
        let (source, control) = envelope(tone(10), Ticket::default(), wake);
        drain(source);
        assert!(control.take_signals().contains(Signals::FINISHED));
        assert_eq!(heard.len(), 1);
    }

    #[rstest]
    fn a_seek_rebases_the_published_position() {
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (mut source, control) = envelope(tone(200), Ticket::default(), wake);
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
        let (mut source, mut control) = envelope(tone(100), Ticket::default(), wake);
        control.ramp(Ramp {
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
            frames: 4410,
        });
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
