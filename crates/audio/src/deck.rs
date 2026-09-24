pub(crate) mod output;
pub(crate) mod source;

use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::Sender as EventSender;
use kernel::{AudioEvent, AudioFailure, Message, Playback};
pub(crate) use source::Landed;

use crate::{
    deck::{
        output::{Output, open_output_stream},
        source::{DeckSource, PreloadRequest},
    },
    device::{StreamFaults, list_output_devices},
    error::device_fault,
    tap::Ring,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reopening {
    pub(crate) device: Option<String>,
    pub(crate) position: Duration,
    pub(crate) playback: Playback,
    pub(crate) opened: DeviceOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceOpen {
    FellBack,
    AsRequested,
}

pub(crate) struct Deck {
    pub(crate) output: Option<Output>,
    source: DeckSource,
    events: EventSender<Message>,
    faults: StreamFaults,
    ring: Arc<Ring>,
}

impl Deck {
    pub(crate) fn new(events: EventSender<Message>, ring: Arc<Ring>) -> Self {
        Self {
            output: None,
            source: DeckSource::new(),
            events,
            faults: StreamFaults::default(),
            ring,
        }
    }

    pub(crate) fn send(&self, event: AudioEvent) {
        let _ = self.events.send(Message::Audio(event));
    }

    pub(crate) fn poll_fault(&self) -> Option<AudioFailure> {
        self.faults.take()
    }

    pub(crate) fn silence(&mut self) {
        self.drop_preload();
        self.output = None;
        self.source.clear_staged();
    }

    pub(crate) fn open(
        &mut self,
        device: Option<String>,
        speed: f32,
    ) -> Result<Reopening, AudioFailure> {
        let opened =
            open_output_stream(device, self.faults.raised()).map_err(device_fault)?;
        let (position, playback) = self
            .output
            .as_ref()
            .map_or((Duration::ZERO, Playback::Playing), Output::at);
        let mut output = Output::with_stream(opened.stream, speed);
        output.listen(Arc::clone(&self.ring));
        self.output = Some(output);
        Ok(Reopening {
            device: opened.device,
            position,
            playback,
            opened: opened.opened,
        })
    }

    pub(crate) fn spawn_decode(&mut self, path: std::path::PathBuf) {
        self.source.spawn_decode(path);
    }

    pub(crate) fn start_preload(&mut self, request: PreloadRequest) {
        self.source.start_preload(request);
    }

    pub(crate) fn drop_preload(&mut self) {
        self.source.drop_preload();
        if let Some(output) = self.output.as_mut() {
            output.preload = None;
        }
    }

    pub(crate) fn clear_staged(&mut self) {
        self.source.clear_staged();
    }

    pub(crate) fn poll_decode(
        &mut self,
    ) -> Option<Result<Option<Duration>, AudioFailure>> {
        self.source.poll_decode()
    }

    pub(crate) fn poll_preload(&mut self) -> Option<Result<Landed, AudioFailure>> {
        self.source.poll_preload(self.output.as_mut())
    }

    pub(crate) fn append_staged(&mut self) {
        self.source.append_staged(self.output.as_mut());
    }

    pub(crate) fn promote(&mut self) {
        if let Some(output) = self.output.as_mut() {
            output.promote();
        }
    }

    pub(crate) fn observe(&self) -> Option<(usize, Duration)> {
        self.output
            .as_ref()
            .map(|output| (output.sink.len(), output.sink.get_pos()))
    }

    pub(crate) fn primary(&self) -> Option<&rodio::Sink> {
        self.output.as_ref().map(|output| &output.sink)
    }

    pub(crate) fn outgoing(&self) -> Option<&rodio::Sink> {
        self.output
            .as_ref()
            .and_then(|output| output.outgoing.as_ref())
    }

    pub(crate) fn sinks(&self) -> impl Iterator<Item = &rodio::Sink> {
        self.output.iter().flat_map(Output::sinks)
    }

    pub(crate) fn list_devices(&self) {
        let events = self.events.clone();
        let builder = thread::Builder::new().name("audio-devices".into());
        let _ = builder.spawn(move || {
            let _ = events.send(Message::Audio(AudioEvent::DevicesLoaded(
                list_output_devices(),
            )));
        });
    }
}
