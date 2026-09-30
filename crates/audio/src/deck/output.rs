use std::time::Duration;

use crossbeam_channel::Sender;
use kernel::{Playback, domain::OutputDevice};
use rodio::{Source, mixer::MixerSource, source::Zero};

use crate::{
    deck::{DeckEvent, DeviceChoice, envelope::Envelope},
    device::open_stream,
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
    wake: &Sender<DeckEvent>,
) -> Result<OpenedOutput, DeviceError> {
    match open_stream(&device, wake) {
        Ok(stream) => Ok(OpenedOutput {
            stream,
            device,
            opened: DeviceChoice::Requested,
        }),
        Err(DeviceError::NotFound { .. }) => {
            open_stream(&OutputDevice::SystemDefault, wake).map(|stream| OpenedOutput {
                stream,
                device: OutputDevice::SystemDefault,
                opened: DeviceChoice::FellBack,
            })
        }
        Err(error) => Err(error),
    }
}

impl Output {
    pub(crate) fn with_stream(stream: rodio::OutputStream, speed: f32) -> Self {
        let channels = stream.config().channel_count();
        let rate = stream.config().sample_rate();
        let (mix, mix_source) = rodio::mixer::mixer(channels, rate);
        mix.add(Zero::new(channels, rate));
        let primary = rodio::Sink::connect_new(&mix);
        primary.set_speed(speed);
        primary.pause();
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

    fn fresh_sink(&self, speed: f32) -> rodio::Sink {
        let primary = rodio::Sink::connect_new(&self.mix);
        primary.set_speed(speed);
        primary.pause();
        primary
    }

    pub(crate) fn swap_sink(&mut self, speed: f32) {
        self.primary = self.fresh_sink(speed);
    }

    pub(crate) fn retire_sink(&mut self, speed: f32) {
        let primary = self.fresh_sink(speed);
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

    pub(crate) fn stage<S>(&mut self, source: Envelope<S>, speed: f32)
    where
        S: Source + Send + 'static,
    {
        let next = rodio::Sink::connect_new(&self.mix);
        next.set_speed(speed);
        next.set_volume(0.0);
        next.append(source);
        next.pause();
        self.incoming = Some(next);
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        std::iter::once(&self.primary)
            .chain(self.incoming.as_ref())
            .chain(self.outgoing.as_ref())
    }
}
