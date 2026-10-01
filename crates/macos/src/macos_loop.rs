#![forbid(unsafe_code)]

use std::{
    iter,
    ops::ControlFlow,
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, bounded, never};
use kernel::{MacosCmd, MacosEvent, Outbox, SendError, Track, update::Machine};
use objc2::rc::{Retained, autoreleasepool};
use objc2_media_player::MPMediaItemArtwork;

use crate::{
    clock::PanelClock,
    core_audio::{HardwareWatch, default_output_device, read_volume, write_volume},
    cover::{
        CoverBytes,
        CoverEffect,
        CoverMessage,
        CoverReader,
        CoverState,
        CoverWorker,
        artwork,
    },
    hardware_state::{HardwareMessage, HardwareState, VolumeMessage},
    now_playing::{Panel, publish},
};

#[derive(Debug)]
pub struct MacosLoop {
    now_playing: Option<Arc<Track>>,
    artwork: Option<Retained<MPMediaItemArtwork>>,
    cover: CoverState,
    cover_worker: Option<CoverWorker>,
    covers_read: Receiver<CoverBytes>,
    clock: PanelClock,
    hardware: HardwareState,
    watch: Option<HardwareWatch>,
    notified: Receiver<()>,
}

impl MacosLoop {
    #[must_use]
    pub fn new(read_cover: CoverReader) -> Self {
        let (cover_worker, covers_read) = match CoverWorker::spawn(read_cover) {
            Ok((worker, covers_read)) => (Some(worker), covers_read),
            Err(_) => (None, never()),
        };
        let (notify, hardware_changed) = bounded(1);
        let (watch, notified) = HardwareWatch::new(notify)
            .map_or_else(|_| (None, never()), |watch| (Some(watch), hardware_changed));
        Self {
            now_playing: None,
            artwork: None,
            cover: CoverState::default(),
            cover_worker,
            covers_read,
            clock: PanelClock::new(Instant::now()),
            hardware: HardwareState::default(),
            watch,
            notified,
        }
    }

    pub fn run<O: Outbox<MacosEvent>>(
        mut self,
        commands: &Receiver<MacosCmd>,
        outbox: &O,
    ) {
        autoreleasepool(|_| self.publish(Instant::now()));
        if autoreleasepool(|_| self.poll(outbox)).is_break() {
            return;
        }
        while self.select_once(commands, outbox).is_continue() {}
    }

    fn select_once<O: Outbox<MacosEvent>>(
        &mut self,
        commands: &Receiver<MacosCmd>,
        outbox: &O,
    ) -> ControlFlow<()> {
        let mut select = Select::new();
        let command_case = select.recv(commands);
        let hardware_case = select.recv(&self.notified);
        let cover_case = select.recv(&self.covers_read);
        let selected = select.select();
        let index = selected.index();
        if index == command_case {
            let Ok(first) = selected.recv(commands) else {
                return ControlFlow::Break(());
            };
            self.drain_commands(first, commands);
            ControlFlow::Continue(())
        } else if index == hardware_case {
            let Ok(()) = selected.recv(&self.notified) else {
                return ControlFlow::Break(());
            };
            autoreleasepool(|_| self.poll(outbox))
        } else if index == cover_case {
            let Ok(bytes) = selected.recv(&self.covers_read) else {
                return ControlFlow::Break(());
            };
            autoreleasepool(|_| self.cover_read(bytes));
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn drain_commands(&mut self, first: MacosCmd, commands: &Receiver<MacosCmd>) {
        let batch = iter::once(first).chain(commands.try_iter()).collect();
        autoreleasepool(|_| {
            keep_last_volume(batch).into_iter().for_each(|command| {
                self.perform(command, Instant::now());
            });
        });
    }

    fn perform(&mut self, command: MacosCmd, now: Instant) {
        match command {
            MacosCmd::NowPlaying(now_playing) => self.show(now_playing, now),
            MacosCmd::PlaybackState(playback) => {
                self.clock = self.clock.change_playback(playback, now);
                self.publish(now);
            }
            MacosCmd::PlaybackPosition(at) => {
                self.clock = self.clock.seek(at, now);
                self.publish(now);
            }
            MacosCmd::Volume(volume) => {
                if write_volume(default_output_device(), volume).is_ok() {
                    let Ok(_) =
                        self.hardware.echo.update(VolumeMessage::Written(volume));
                }
            }
        }
    }

    fn show(&mut self, now_playing: Option<Arc<Track>>, now: Instant) {
        let track = now_playing
            .as_deref()
            .map(|track| track.path().to_path_buf());
        let Ok(effect) = self.cover.update(CoverMessage::TrackShown(track));
        self.apply_cover_effect(effect);
        self.now_playing = now_playing;
        self.clock = self.clock.seek(Duration::ZERO, now);
        self.publish(now);
    }

    fn cover_read(&mut self, bytes: CoverBytes) {
        let Ok(effect) = self.cover.update(CoverMessage::Read(bytes));
        self.apply_cover_effect(effect);
        self.publish(Instant::now());
    }

    fn apply_cover_effect(&mut self, effect: CoverEffect) {
        match effect {
            CoverEffect::Nothing => {}
            CoverEffect::Clear => self.artwork = None,
            CoverEffect::Request(track) => {
                self.artwork = None;
                if let Some(worker) = &self.cover_worker {
                    worker.request(track);
                }
            }
            CoverEffect::Show(bytes) => self.artwork = artwork(&bytes),
        }
    }

    fn publish(&self, now: Instant) {
        let panel = Panel {
            track: self.now_playing.as_deref(),
            clock: self.clock,
            artwork: self.artwork.as_deref(),
        };
        publish(panel, now);
    }

    fn poll<O: Outbox<MacosEvent>>(&mut self, outbox: &O) -> ControlFlow<()> {
        let Some(watch) = &mut self.watch else {
            return ControlFlow::Continue(());
        };
        let current = default_output_device();
        let polled = HardwareMessage {
            tracked_device: watch.tracked_device(),
            current_device: current,
            volume: read_volume(current),
        };
        let Ok(mut effect) = self.hardware.update(polled);
        if let Some(device) = effect.rebind
            && let Err(error) = watch.rebind_to(device)
        {
            effect
                .events
                .push(MacosEvent::HardwareWatchError(error.to_string()));
        }
        send_events(effect.events, outbox)
    }
}

fn send_events<O: Outbox<MacosEvent>>(
    events: Vec<MacosEvent>,
    outbox: &O,
) -> ControlFlow<()> {
    for event in events {
        match outbox.send(event) {
            Ok(()) | Err(SendError::Full) => {}
            Err(SendError::Closed) => return ControlFlow::Break(()),
        }
    }
    ControlFlow::Continue(())
}

fn keep_last_volume(commands: Vec<MacosCmd>) -> Vec<MacosCmd> {
    let last_volume = commands.iter().rev().find_map(|command| match command {
        MacosCmd::Volume(volume) => Some(*volume),
        MacosCmd::NowPlaying(_)
        | MacosCmd::PlaybackState(_)
        | MacosCmd::PlaybackPosition(_) => None,
    });
    commands
        .into_iter()
        .filter(|command| !matches!(command, MacosCmd::Volume(_)))
        .chain(last_volume.map(MacosCmd::Volume))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, ops::ControlFlow};

    use kernel::{Bounded, MacosCmd, MacosEvent, Outbox, Percent, Playback, SendError};
    use rstest::rstest;

    use crate::macos_loop::{keep_last_volume, send_events};

    fn volume(value: u8) -> MacosCmd {
        MacosCmd::Volume(Percent::clamped(value))
    }

    #[rstest]
    #[case::three_volumes(
        vec![volume(10), volume(20), volume(30)],
        vec![volume(30)]
    )]
    #[case::volumes_among_other_commands(
        vec![
            MacosCmd::PlaybackState(Playback::Playing),
            volume(10),
            MacosCmd::PlaybackState(Playback::Paused),
            volume(20),
        ],
        vec![
            MacosCmd::PlaybackState(Playback::Playing),
            MacosCmd::PlaybackState(Playback::Paused),
            volume(20),
        ]
    )]
    #[case::no_volume_at_all(
        vec![MacosCmd::PlaybackState(Playback::Playing)],
        vec![MacosCmd::PlaybackState(Playback::Playing)]
    )]
    fn keep_last_volume_puts_the_last_volume_last_and_the_rest_in_order(
        #[case] commands: Vec<MacosCmd>,
        #[case] applied: Vec<MacosCmd>,
    ) {
        assert_eq!(keep_last_volume(commands), applied);
    }

    struct FakeOutbox {
        sent: RefCell<Vec<MacosEvent>>,
        send_result: Result<(), SendError>,
    }

    impl Outbox<MacosEvent> for FakeOutbox {
        fn send(&self, event: MacosEvent) -> Result<(), SendError> {
            self.sent.borrow_mut().push(event);
            self.send_result
        }
    }

    #[rstest]
    #[case::sent(Ok(()), ControlFlow::Continue(()), 2)]
    #[case::full(Err(SendError::Full), ControlFlow::Continue(()), 2)]
    #[case::closed(Err(SendError::Closed), ControlFlow::Break(()), 1)]
    fn send_events_stops_only_on_a_closed_outbox(
        #[case] send_result: Result<(), SendError>,
        #[case] flow: ControlFlow<()>,
        #[case] sent_count: usize,
    ) {
        let events = vec![
            MacosEvent::OutputRouteChanged,
            MacosEvent::OutputRouteChanged,
        ];
        let outbox = FakeOutbox {
            sent: RefCell::new(Vec::new()),
            send_result,
        };

        let observed = send_events(events, &outbox);

        assert_eq!(observed, flow);
        assert_eq!(outbox.sent.borrow().len(), sent_count);
    }
}
