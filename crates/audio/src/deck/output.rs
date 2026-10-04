use std::time::Duration;

use crossbeam_channel::Sender;
use kernel::{
    Playback,
    domain::{OutputDevice, Speed},
};
use rodio::{Source, mixer::MixerSource, source::Zero};

use crate::{
    deck::{DeviceChoice, envelope::Envelope},
    device::open_stream,
    engine::effect::AudioMessage,
    error::DeviceError,
    tap::{Handoff, Tap},
};

pub(crate) struct Output {
    stream: rodio::OutputStream,
    mix: rodio::mixer::Mixer,
    mix_source: Option<MixerSource>,
    pub(crate) primary: rodio::Sink,
    pub(crate) incoming: Option<rodio::Sink>,
    pub(crate) outgoing: Option<rodio::Sink>,
}

pub(crate) struct OpenedOutput {
    pub(crate) stream: rodio::OutputStream,
    pub(crate) device: OutputDevice,
    pub(crate) opened: DeviceChoice,
}

pub(crate) fn open_output_stream(
    device: OutputDevice,
    sender: &Sender<AudioMessage>,
) -> Result<OpenedOutput, DeviceError> {
    match open_stream(&device, sender) {
        Ok(stream) => Ok(OpenedOutput {
            stream,
            device,
            opened: DeviceChoice::Requested,
        }),
        Err(DeviceError::NotFound(_)) => {
            open_stream(&OutputDevice::SystemDefault, sender).map(|stream| {
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
    pub(crate) fn with_stream(stream: rodio::OutputStream, speed: Speed) -> Self {
        let channels = stream.config().channel_count();
        let rate = stream.config().sample_rate();
        let (mix, mix_source) = rodio::mixer::mixer(channels, rate);
        mix.add(Zero::new(channels, rate));
        let primary = fresh_sink(&mix, speed);
        Self {
            stream,
            mix,
            mix_source: Some(mix_source),
            primary,
            incoming: None,
            outgoing: None,
        }
    }

    pub(crate) fn listen(&mut self, spectrum: &Handoff) {
        let Some(mix_source) = self.mix_source.take() else {
            return;
        };
        self.stream.mixer().add(Tap::new(mix_source, spectrum));
    }

    pub(crate) fn swap_sink(&mut self, speed: Speed) {
        self.primary = fresh_sink(&self.mix, speed);
    }

    pub(crate) fn retire_sink(&mut self, speed: Speed) {
        let primary = fresh_sink(&self.mix, speed);
        self.outgoing = Some(std::mem::replace(&mut self.primary, primary));
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
        if let Some(incoming) = self.incoming.take() {
            self.primary = incoming;
        }
    }

    pub(crate) fn append<S>(&self, source: Envelope<S>)
    where
        S: Source + Send + 'static,
    {
        self.primary.append(source);
    }

    pub(crate) fn stage<S>(&mut self, source: Envelope<S>, speed: Speed)
    where
        S: Source + Send + 'static,
    {
        let next = fresh_sink(&self.mix, speed);
        next.set_volume(0.0);
        next.append(source);
        self.incoming = Some(next);
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        std::iter::once(&self.primary)
            .chain(self.incoming.as_ref())
            .chain(self.outgoing.as_ref())
    }
}
