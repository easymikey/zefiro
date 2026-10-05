use std::time::Duration;

use crossbeam_channel::Sender;
use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, revision::Revision, speed::Speed},
};
use rodio::{Source, source::Zero};

use crate::{
    deck::envelope::{Envelope, EnvelopeControl},
    device::{OutputLoss, open_stream},
    engine::message::{AudioMessage, DeviceChoice, SinkRole},
    error::DeviceError,
    gain::Gain,
    tap::{Handoff, Tap},
};

pub(crate) struct Fader {
    pub(crate) sink: rodio::Sink,
    pub(crate) control: EnvelopeControl,
}

pub(crate) struct Output {
    _stream: Box<dyn std::any::Any>,
    mix: rodio::mixer::Mixer,
    pub(crate) primary: rodio::Sink,
    pub(crate) control: Option<EnvelopeControl>,
    pub(crate) queued_control: Option<EnvelopeControl>,
    pub(crate) incoming_fader: Option<Fader>,
    pub(crate) outgoing_fader: Option<Fader>,
}

pub(crate) struct OpenedOutput {
    pub(crate) stream: rodio::OutputStream,
    pub(crate) device: OutputDevice,
    pub(crate) opened: DeviceChoice,
}

pub(crate) fn open_output_stream(
    device: OutputDevice,
    sender: &Sender<AudioMessage>,
    lost: &OutputLoss,
) -> Result<OpenedOutput, DeviceError> {
    match open_stream(&device, sender, lost) {
        Ok(stream) => Ok(OpenedOutput {
            stream,
            device,
            opened: DeviceChoice::Requested,
        }),
        Err(DeviceError::NotFound(_)) => {
            open_stream(&OutputDevice::SystemDefault, sender, lost).map(|stream| {
                OpenedOutput {
                    stream,
                    device: OutputDevice::SystemDefault,
                    opened: DeviceChoice::FellBack,
                }
            })
        }
        Err(error) => Err(error),
    }
}

fn fresh_sink(mix: &rodio::mixer::Mixer, speed: Speed) -> rodio::Sink {
    let sink = rodio::Sink::connect_new(mix);
    sink.set_speed(speed.get());
    sink.pause();
    sink
}

impl Output {
    pub(crate) fn with_stream(
        stream: rodio::OutputStream,
        speed: Speed,
        spectrum: &Handoff,
    ) -> Self {
        let channels = stream.config().channel_count();
        let rate = stream.config().sample_rate();
        let (mix, mix_source) = rodio::mixer::mixer(channels, rate);
        stream.mixer().add(Tap::new(mix_source, spectrum));
        mix.add(Zero::new(channels, rate));
        let primary = fresh_sink(&mix, speed);
        Self {
            _stream: Box::new(stream),
            mix,
            primary,
            control: None,
            queued_control: None,
            incoming_fader: None,
            outgoing_fader: None,
        }
    }

    pub(crate) fn swap_sink(&mut self, speed: Speed) {
        self.outgoing_fader = None;
        self.primary = fresh_sink(&self.mix, speed);
        self.control = None;
    }

    pub(crate) fn retire_sink(&mut self, speed: Speed) {
        let primary = fresh_sink(&self.mix, speed);
        let sink = std::mem::replace(&mut self.primary, primary);
        self.outgoing_fader =
            self.control.take().map(|control| Fader { sink, control });
    }

    pub(crate) fn at(&self) -> (Duration, Playback) {
        let playback = if self.primary.is_paused() {
            Playback::Paused
        } else {
            Playback::Playing
        };
        (self.primary.get_pos(), playback)
    }

    pub(crate) fn promote(&mut self) {
        if let Some(Fader { sink, control }) = self.incoming_fader.take() {
            self.primary = sink;
            self.control = Some(control);
        }
    }

    pub(crate) fn append<S>(&self, source: Envelope<S>)
    where
        S: Source + Send + 'static,
    {
        self.primary.append(source);
    }

    pub(crate) fn stage<S>(
        &mut self,
        (source, control): (Envelope<S>, EnvelopeControl),
        speed: Speed,
    ) where
        S: Source + Send + 'static,
    {
        let sink = fresh_sink(&self.mix, speed);
        sink.set_volume(Gain::SILENCE.amplitude());
        sink.append(source);
        self.incoming_fader = Some(Fader { sink, control });
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        std::iter::once(&self.primary)
            .chain(self.incoming_fader.as_ref().map(|fader| &fader.sink))
            .chain(self.outgoing_fader.as_ref().map(|fader| &fader.sink))
    }

    #[must_use]
    pub(crate) fn holding(&self, revision: Revision) -> Option<&EnvelopeControl> {
        [
            self.control.as_ref(),
            self.queued_control.as_ref(),
            self.incoming_fader.as_ref().map(|fader| &fader.control),
            self.outgoing_fader.as_ref().map(|fader| &fader.control),
        ]
        .into_iter()
        .flatten()
        .find(|control| control.revision() == revision)
    }

    #[must_use]
    pub(crate) fn role(&self, revision: Revision) -> Option<SinkRole> {
        let holds = |control: &EnvelopeControl| control.revision() == revision;
        if self.control.as_ref().is_some_and(holds) {
            Some(SinkRole::Primary)
        } else if self
            .incoming_fader
            .as_ref()
            .is_some_and(|fader| holds(&fader.control))
        {
            Some(SinkRole::Incoming)
        } else if self
            .outgoing_fader
            .as_ref()
            .is_some_and(|fader| holds(&fader.control))
        {
            Some(SinkRole::Outgoing)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use kernel::domain::speed::Speed;

    use crate::deck::output::{Output, fresh_sink};

    pub(crate) fn detached_output() -> Output {
        let (mix, _mix_source) = rodio::mixer::mixer(1, 44_100);
        let primary = fresh_sink(&mix, Speed::default());
        Output {
            _stream: Box::new(()),
            mix,
            primary,
            control: None,
            queued_control: None,
            incoming_fader: None,
            outgoing_fader: None,
        }
    }
}
