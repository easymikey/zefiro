#![forbid(unsafe_code)]

use std::{
    iter,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, SelectedOperation, bounded};
use kernel::{
    MacosCmd,
    MacosEvent,
    NowPlaying,
    Outbox,
    Percent,
    SendError,
    update::{Machine, Rejected},
};
use objc2::rc::autoreleasepool;
use objc2_core_audio::AudioObjectID;

use crate::{
    audio_hardware::{HardwareWatch, default_output_device},
    clock::PanelClock,
    cover::{Cover, CoverBytes, CoverReader, CoverWorker},
    cover_slot::{CoverEffect, CoverMessage, CoverSlot},
    echo::{VolumeEcho, VolumeEffect, VolumeMessage},
    now_playing::{Panel, publish},
    output::{DefaultOutput, OutputEffect, OutputMessage},
    volume::{read_volume, write_volume},
};

#[derive(Debug)]
pub struct MacosLoop {
    read_cover: CoverReader,
    now_playing: NowPlaying,
    artwork: Option<Cover>,
    slot: CoverSlot,
    cover_worker: Option<CoverWorker>,
    clock: PanelClock,
    hardware: HardwareState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stop,
}

struct HardwareChannel<'a> {
    watch: &'a mut HardwareWatch,
    notified: &'a Receiver<()>,
}

struct LoopChannels<'a, O> {
    commands: &'a Receiver<MacosCmd>,
    hardware: Option<HardwareChannel<'a>>,
    arrivals: Option<&'a Receiver<CoverBytes>>,
    outbox: &'a O,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RebindDecision {
    Unchanged,
    Changed(AudioObjectID),
}

fn rebind_decision(tracked: AudioObjectID, current: AudioObjectID) -> RebindDecision {
    if tracked == current {
        RebindDecision::Unchanged
    } else {
        RebindDecision::Changed(current)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HardwareMessage {
    pub(crate) tracked_device: AudioObjectID,
    pub(crate) current_device: AudioObjectID,
    pub(crate) volume: Option<Percent>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HardwareEffect {
    pub(crate) rebind: Option<AudioObjectID>,
    pub(crate) events: Vec<MacosEvent>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HardwareState {
    echo: VolumeEcho,
    output: DefaultOutput,
}

impl HardwareState {
    fn record_written(&mut self, volume: Percent) {
        let Ok(_) = self.echo.update(VolumeMessage::Written(volume));
    }
}

impl Machine for HardwareState {
    type Message = HardwareMessage;
    type Error = std::convert::Infallible;
    type Effect = HardwareEffect;

    fn transition(
        self,
        read: HardwareMessage,
    ) -> Result<(Self, HardwareEffect), Rejected<Self>> {
        let mut events = Vec::new();
        let echo = read.volume.map_or(self.echo, |volume| {
            let Ok((echo, effect)) =
                self.echo.transition(VolumeMessage::Polled(volume));
            if let VolumeEffect::Changed(reported) = effect {
                events.push(MacosEvent::Volume(reported));
            }
            echo
        });
        let Ok((output, effect)) =
            self.output.transition(OutputMessage(read.current_device));
        if let OutputEffect::Changed = effect {
            events.push(MacosEvent::OutputRouteChanged);
        }
        let rebind = match rebind_decision(read.tracked_device, read.current_device) {
            RebindDecision::Unchanged => None,
            RebindDecision::Changed(device) => Some(device),
        };
        Ok((Self { echo, output }, HardwareEffect { rebind, events }))
    }
}

impl MacosLoop {
    #[must_use]
    pub fn new(read_cover: CoverReader) -> Self {
        Self {
            read_cover,
            now_playing: NowPlaying::Cleared,
            artwork: None,
            slot: CoverSlot::default(),
            cover_worker: None,
            clock: PanelClock::new(Instant::now()),
            hardware: HardwareState::default(),
        }
    }

    pub fn run<O: Outbox<MacosEvent>>(
        mut self,
        commands: &Receiver<MacosCmd>,
        outbox: &O,
    ) {
        autoreleasepool(|_| self.publish(Instant::now()));
        let arrivals = self.spawn_cover_worker();
        let (notify, notified) = bounded(1);
        match HardwareWatch::new(notify) {
            Ok(mut watch) => {
                autoreleasepool(|_| self.poll(&mut watch, outbox));
                self.serve(LoopChannels {
                    commands,
                    hardware: Some(HardwareChannel {
                        watch: &mut watch,
                        notified: &notified,
                    }),
                    arrivals: arrivals.as_ref(),
                    outbox,
                });
            }
            Err(_) => self.serve(LoopChannels {
                commands,
                hardware: None,
                arrivals: arrivals.as_ref(),
                outbox,
            }),
        }
    }

    fn spawn_cover_worker(&mut self) -> Option<Receiver<CoverBytes>> {
        match CoverWorker::spawn(self.read_cover) {
            Ok((worker, arrivals)) => {
                self.cover_worker = Some(worker);
                Some(arrivals)
            }
            Err(_) => None,
        }
    }

    fn serve<O: Outbox<MacosEvent>>(&mut self, mut session: LoopChannels<'_, O>) {
        loop {
            if let Flow::Stop = self.select_once(&mut session) {
                return;
            }
        }
    }

    fn select_once<O: Outbox<MacosEvent>>(
        &mut self,
        session: &mut LoopChannels<'_, O>,
    ) -> Flow {
        let mut select = Select::new();
        let command_case = select.recv(session.commands);
        let hardware_case = session
            .hardware
            .as_ref()
            .map(|hardware| select.recv(hardware.notified));
        let arrival = session.arrivals.map(|arrivals| select.recv(arrivals));
        let selected = select.select();
        let index = selected.index();
        if index == command_case {
            self.on_command(selected, session.commands)
        } else if Some(index) == hardware_case {
            self.on_hardware_changed(selected, session)
        } else if Some(index) == arrival {
            self.on_cover_read(selected, session)
        } else {
            Flow::Stop
        }
    }

    fn on_command(
        &mut self,
        selected: SelectedOperation<'_>,
        commands: &Receiver<MacosCmd>,
    ) -> Flow {
        selected.recv(commands).map_or(Flow::Stop, |first| {
            self.drain_commands(first, commands);
            Flow::Continue
        })
    }

    fn on_hardware_changed<O: Outbox<MacosEvent>>(
        &mut self,
        selected: SelectedOperation<'_>,
        session: &mut LoopChannels<'_, O>,
    ) -> Flow {
        let Some(hardware) = &mut session.hardware else {
            return Flow::Stop;
        };
        match selected.recv(hardware.notified) {
            Ok(()) => autoreleasepool(|_| self.poll(hardware.watch, session.outbox)),
            Err(_) => Flow::Stop,
        }
    }

    fn on_cover_read<O: Outbox<MacosEvent>>(
        &mut self,
        selected: SelectedOperation<'_>,
        session: &LoopChannels<'_, O>,
    ) -> Flow {
        let Some(arrivals) = session.arrivals else {
            return Flow::Stop;
        };
        selected.recv(arrivals).map_or(Flow::Stop, |bytes| {
            autoreleasepool(|_| self.cover_read(bytes));
            Flow::Continue
        })
    }

    fn drain_commands(&mut self, first: MacosCmd, commands: &Receiver<MacosCmd>) {
        let batch = iter::once(first).chain(commands.try_iter()).collect();
        autoreleasepool(|_| {
            coalesced(batch).into_iter().for_each(|command| {
                self.perform(command, Instant::now());
            });
        });
    }

    fn perform(&mut self, command: MacosCmd, now: Instant) {
        match command {
            MacosCmd::NowPlaying(now_playing) => self.show(now_playing, now),
            MacosCmd::PlaybackState(playback) => {
                self.clock = self.clock.with_playback(playback, now);
                self.publish(now);
            }
            MacosCmd::PlaybackPosition(at) => {
                self.clock = self.clock.seek(at, now);
                self.publish(now);
            }
            MacosCmd::Volume(volume) => {
                if write_volume(default_output_device(), volume).is_ok() {
                    self.hardware.record_written(volume);
                }
            }
        }
    }

    fn show(&mut self, now_playing: NowPlaying, now: Instant) {
        let track = match &now_playing {
            NowPlaying::Track { path, .. } => Some(path.clone()),
            NowPlaying::Cleared => None,
        };
        let Ok(effect) = self.slot.update(CoverMessage::TrackShown(track));
        self.apply_cover_effect(effect);
        self.now_playing = now_playing;
        self.clock = self.clock.seek(Duration::ZERO, now);
        self.publish(now);
    }

    fn cover_read(&mut self, bytes: CoverBytes) {
        let Ok(effect) = self.slot.update(CoverMessage::Arrived(bytes));
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
            CoverEffect::Show(bytes) => self.artwork = Some(Cover::from_bytes(&bytes)),
        }
    }

    fn publish(&self, now: Instant) {
        let panel = Panel {
            showing: &self.now_playing,
            clock: self.clock,
            artwork: self.artwork.as_ref().and_then(Cover::artwork),
        };
        publish(panel, now);
    }

    fn poll<O: Outbox<MacosEvent>>(
        &mut self,
        watch: &mut HardwareWatch,
        outbox: &O,
    ) -> Flow {
        let current = default_output_device();
        let read = HardwareMessage {
            tracked_device: watch.tracked_device(),
            current_device: current,
            volume: read_volume(current),
        };
        let Ok(mut effect) = self.hardware.update(read);
        if let Some(device) = effect.rebind
            && let Err(failure) = watch.rebind_to(device)
        {
            effect
                .events
                .push(MacosEvent::HardwareWatchError(failure.to_string()));
        }
        send_events(effect.events, outbox)
    }
}

fn send_events<O: Outbox<MacosEvent>>(events: Vec<MacosEvent>, outbox: &O) -> Flow {
    for event in events {
        match outbox.send(event) {
            Ok(()) | Err(SendError::Full) => {}
            Err(SendError::Closed) => return Flow::Stop,
        }
    }
    Flow::Continue
}

fn coalesced(commands: Vec<MacosCmd>) -> Vec<MacosCmd> {
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
    use std::cell::RefCell;

    use kernel::{
        Bounded,
        MacosCmd,
        MacosEvent,
        Outbox,
        Percent,
        Playback,
        SendError,
        update::Machine,
    };
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::macos_loop::{
        Flow,
        HardwareEffect,
        HardwareMessage,
        HardwareState,
        RebindDecision,
        coalesced,
        rebind_decision,
        send_events,
    };

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
    fn coalesced_keeps_the_last_volume_last_and_the_rest_in_order(
        #[case] commands: Vec<MacosCmd>,
        #[case] applied: Vec<MacosCmd>,
    ) {
        assert_eq!(coalesced(commands), applied);
    }

    fn read(
        devices: (AudioObjectID, AudioObjectID),
        volume: Option<u8>,
    ) -> HardwareMessage {
        let (tracked_device, current_device) = devices;
        HardwareMessage {
            tracked_device,
            current_device,
            volume: volume.map(Percent::clamped),
        }
    }

    #[rstest]
    #[case::same_device(1, 1, RebindDecision::Unchanged)]
    #[case::a_different_device(1, 2, RebindDecision::Changed(2))]
    fn rebind_decision_only_fires_on_a_different_device(
        #[case] tracked: AudioObjectID,
        #[case] current: AudioObjectID,
        #[case] decision: RebindDecision,
    ) {
        assert_eq!(rebind_decision(tracked, current), decision);
    }

    #[rstest]
    #[case::a_volume_only_event_never_rebinds(
        vec![read((1, 1), None)],
        vec![HardwareEffect::default()]
    )]
    #[case::a_changed_output_route_is_reported_once(
        vec![read((1, 1), None), read((1, 2), None), read((2, 2), None)],
        vec![
            HardwareEffect::default(),
            HardwareEffect {
                rebind: Some(2),
                events: vec![MacosEvent::OutputRouteChanged],
            },
            HardwareEffect::default(),
        ]
    )]
    #[case::the_first_volume_poll_is_always_reported(
        vec![read((1, 1), Some(30)), read((1, 1), Some(30))],
        vec![
            HardwareEffect {
                rebind: None,
                events: vec![MacosEvent::Volume(Percent::clamped(30))],
            },
            HardwareEffect::default(),
        ]
    )]
    fn a_hardware_read_reports_only_what_changed(
        #[case] reads: Vec<HardwareMessage>,
        #[case] effects: Vec<HardwareEffect>,
    ) {
        let mut hardware = HardwareState::default();
        let observed: Vec<HardwareEffect> = reads
            .into_iter()
            .map(|read| {
                let Ok(effect) = hardware.update(read);
                effect
            })
            .collect();
        assert_eq!(observed, effects);
    }

    struct FakeOutbox {
        sent: RefCell<Vec<MacosEvent>>,
        delivery: Result<(), SendError>,
    }

    impl Outbox<MacosEvent> for FakeOutbox {
        fn send(&self, event: MacosEvent) -> Result<(), SendError> {
            self.sent.borrow_mut().push(event);
            self.delivery
        }
    }

    #[rstest]
    #[case::sent(Ok(()), Flow::Continue, 2)]
    #[case::congested(Err(SendError::Full), Flow::Continue, 2)]
    #[case::closed(Err(SendError::Closed), Flow::Stop, 1)]
    fn deliver_stops_only_on_a_closed_outbox(
        #[case] delivery: Result<(), SendError>,
        #[case] flow: Flow,
        #[case] delivered: usize,
    ) {
        let events = vec![
            MacosEvent::OutputRouteChanged,
            MacosEvent::OutputRouteChanged,
        ];
        let outbox = FakeOutbox {
            sent: RefCell::new(Vec::new()),
            delivery,
        };

        let observed = send_events(events, &outbox);

        assert_eq!(observed, flow);
        assert_eq!(outbox.sent.borrow().len(), delivered);
    }
}
