use std::time::Duration;

use crossbeam_channel::Sender;
use kernel::{Playback, domain::DeviceName};
use rodio::{Source, mixer::MixerSource, source::Zero};

use crate::{
    deck::{DeckEvent, DeviceOpen, envelope::Envelope},
    device::open_stream,
    error::DeviceError,
    tap::{Handoff, Tap},
};

pub(crate) struct Output {
    stream: rodio::OutputStream,
    mix: rodio::mixer::Mixer,
    mix_source: Option<MixerSource>,
    pub(crate) sink: rodio::Sink,
    pub(crate) preload: Option<rodio::Sink>,
    pub(crate) outgoing: Option<rodio::Sink>,
}

pub(crate) struct OpenedStream {
    pub(crate) stream: rodio::OutputStream,
    pub(crate) device: Option<DeviceName>,
    pub(crate) opened: DeviceOpen,
}

pub(crate) fn open_output_stream(
    device: Option<DeviceName>,
    wake: &Sender<DeckEvent>,
) -> Result<OpenedStream, DeviceError> {
    match open_stream(device.as_ref(), wake) {
        Ok(stream) => Ok(OpenedStream {
            stream,
            device,
            opened: DeviceOpen::AsRequested,
        }),
        Err(DeviceError::NotFound { .. }) => {
            open_stream(None, wake).map(|stream| OpenedStream {
                stream,
                device: None,
                opened: DeviceOpen::FellBack,
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
        let sink = rodio::Sink::connect_new(&mix);
        sink.set_speed(speed);
        sink.pause();
        Self {
            stream,
            mix,
            mix_source: Some(mix_source),
            sink,
            preload: None,
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
        let sink = rodio::Sink::connect_new(&self.mix);
        sink.set_speed(speed);
        sink.pause();
        sink
    }

    pub(crate) fn swap_sink(&mut self, speed: f32) {
        self.sink = self.fresh_sink(speed);
    }

    pub(crate) fn retire_sink(&mut self, speed: f32) {
        let sink = self.fresh_sink(speed);
        self.outgoing = Some(std::mem::replace(&mut self.sink, sink));
    }

    pub(crate) fn at(&self) -> (Duration, Playback) {
        let playback = if self.sink.is_paused() {
            Playback::Paused
        } else {
            Playback::Playing
        };
        (self.sink.get_pos(), playback)
    }

    pub(crate) fn promote(&mut self) {
        if let Some(preload) = self.preload.take() {
            self.sink = preload;
        }
    }

    pub(crate) fn append<S>(&self, source: Envelope<S>)
    where
        S: Source + Send + 'static,
    {
        self.sink.append(source);
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
        self.preload = Some(next);
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        std::iter::once(&self.sink)
            .chain(self.preload.as_ref())
            .chain(self.outgoing.as_ref())
    }
}
