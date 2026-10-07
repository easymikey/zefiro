use std::time::Duration;

use kernel::{
    cmd::Playback,
    domain::{revision::Revision, speed::Speed},
};
use rodio::{Source, source::Zero};

use crate::{
    deck::envelope::{Envelope, EnvelopeControl},
    engine::message::SinkRole,
    gain::Gain,
    tap::{SpectrumBuffers, TappedSource},
};

pub(crate) struct Fader {
    pub(crate) sink: rodio::Sink,
    pub(crate) control: EnvelopeControl,
}

pub(crate) struct Output {
    mix: rodio::mixer::Mixer,
    pub(crate) current: rodio::Sink,
    pub(crate) current_control: Option<EnvelopeControl>,
    pub(crate) incoming_control: Option<EnvelopeControl>,
    pub(crate) incoming_fader: Option<Fader>,
    pub(crate) outgoing_fader: Option<Fader>,
}

fn fresh_sink(mix: &rodio::mixer::Mixer, speed: Speed) -> rodio::Sink {
    let sink = rodio::Sink::connect_new(mix);
    sink.set_speed(speed.get());
    sink.pause();
    sink
}

impl Output {
    pub(crate) fn with_stream(
        stream: &rodio::OutputStream,
        speed: Speed,
        spectrum_buffers: &SpectrumBuffers,
    ) -> Self {
        let channels = stream.config().channel_count();
        let rate = stream.config().sample_rate();
        let (mix, mix_source) = rodio::mixer::mixer(channels, rate);
        stream
            .mixer()
            .add(TappedSource::new(mix_source, spectrum_buffers));
        mix.add(Zero::new(channels, rate));
        let current = fresh_sink(&mix, speed);
        Self {
            mix,
            current,
            current_control: None,
            incoming_control: None,
            incoming_fader: None,
            outgoing_fader: None,
        }
    }

    pub(crate) fn swap_current(&mut self, speed: Speed) {
        self.outgoing_fader = None;
        self.current = fresh_sink(&self.mix, speed);
        self.current_control = None;
    }

    pub(crate) fn retire_current(&mut self, speed: Speed) {
        let current = fresh_sink(&self.mix, speed);
        let sink = std::mem::replace(&mut self.current, current);
        self.outgoing_fader = self
            .current_control
            .take()
            .map(|control| Fader { sink, control });
    }

    pub(crate) fn position(&self) -> (Duration, Playback) {
        let playback = if self.current.is_paused() {
            Playback::Paused
        } else {
            Playback::Playing
        };
        (self.current.get_pos(), playback)
    }

    pub(crate) fn promote(&mut self) {
        if let Some(Fader { sink, control }) = self.incoming_fader.take() {
            self.current = sink;
            self.current_control = Some(control);
        }
    }

    pub(crate) fn attach_incoming<S>(
        &self,
        envelope: Envelope<S>,
        speed: Speed,
    ) -> rodio::Sink
    where
        S: Source + Send + 'static,
    {
        let sink = fresh_sink(&self.mix, speed);
        sink.set_volume(Gain::SILENCE.amplitude());
        sink.append(envelope);
        sink
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        std::iter::once(&self.current)
            .chain(self.incoming_fader.as_ref().map(|fader| &fader.sink))
            .chain(self.outgoing_fader.as_ref().map(|fader| &fader.sink))
    }

    #[must_use]
    pub(crate) fn holder(
        &self,
        revision: Revision,
    ) -> Option<(SinkRole, &EnvelopeControl)> {
        [
            (SinkRole::Current, self.current_control.as_ref()),
            (
                SinkRole::Incoming,
                self.incoming_fader.as_ref().map(|fader| &fader.control),
            ),
            (
                SinkRole::Outgoing,
                self.outgoing_fader.as_ref().map(|fader| &fader.control),
            ),
        ]
        .into_iter()
        .find_map(|(role, control)| {
            control
                .filter(|control| control.revision() == revision)
                .map(|control| (role, control))
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use kernel::domain::speed::Speed;

    use crate::deck::output::{Output, fresh_sink};

    pub(crate) fn detached_output() -> Output {
        let (mix, _mix_source) = rodio::mixer::mixer(1, 44_100);
        let current = fresh_sink(&mix, Speed::default());
        Output {
            mix,
            current,
            current_control: None,
            incoming_control: None,
            incoming_fader: None,
            outgoing_fader: None,
        }
    }
}
