#![forbid(unsafe_code)]

use std::{
    iter,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, SelectedOperation, bounded};
use kernel::{
    Delivery,
    NowPlaying,
    Outbox,
    Percent,
    SystemCmd,
    SystemEvent,
    update::{Machine, Never, Rejected},
};
use objc2::rc::autoreleasepool;
use objc2_core_audio::AudioObjectID;

use crate::{
    audio_hardware::{HardwareWatch, current_default_device},
    clock::PanelClock,
    cover::{Cover, CoverBytes, CoverReader, CoverWorker},
    cover_slot::{CoverEffect, CoverMessage, CoverSlot},
    echo::{VolumeEcho, VolumeEffect, VolumeMessage},
    now_playing::{Panel, publish},
    output::{DefaultOutput, OutputEffect, OutputPolled},
    volume::{Written, read_volume, write_volume},
};

#[derive(Debug)]
pub struct SystemLoop {
    read_cover: CoverReader,
    showing: NowPlaying,
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

struct HardwarePair<'a> {
    watch: &'a mut HardwareWatch,
    rings: &'a Receiver<()>,
}

struct Session<'a, O> {
    commands: &'a Receiver<SystemCmd>,
    hardware: Option<HardwarePair<'a>>,
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
pub(crate) struct HardwareRead {
    pub(crate) tracked_device: AudioObjectID,
    pub(crate) current_device: AudioObjectID,
    pub(crate) volume: Option<Percent>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HardwareEffect {
    pub(crate) rebind: Option<AudioObjectID>,
    pub(crate) facts: Vec<SystemEvent>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HardwareState {
    echo: VolumeEcho,
    output: DefaultOutput,
}

impl HardwareState {
    fn note_written(&mut self, volume: Percent) {
        let Ok(_) = self.echo.update(VolumeMessage::Written(volume));
    }
}

impl Machine for HardwareState {
    type Message = HardwareRead;
    type Rejection = Never;
    type Effect = HardwareEffect;

    fn transition(
        self,
        read: HardwareRead,
    ) -> Result<(Self, HardwareEffect), Rejected<Self>> {
        let mut facts = Vec::new();
        let echo = read.volume.map_or(self.echo, |volume| {
            let Ok((echo, effect)) =
                self.echo.transition(VolumeMessage::Polled(volume));
            if let VolumeEffect::Report(reported) = effect {
                facts.push(SystemEvent::Volume(reported));
            }
            echo
        });
        let Ok((output, effect)) =
            self.output.transition(OutputPolled(read.current_device));
        if let OutputEffect::Changed = effect {
            facts.push(SystemEvent::OutputRouteChanged);
        }
        let rebind = match rebind_decision(read.tracked_device, read.current_device) {
            RebindDecision::Unchanged => None,
            RebindDecision::Changed(device) => Some(device),
        };
        Ok((Self { echo, output }, HardwareEffect { rebind, facts }))
    }
}

impl SystemLoop {
    #[must_use]
    pub fn new(read_cover: CoverReader) -> Self {
        Self {
            read_cover,
            showing: NowPlaying::Cleared,
            artwork: None,
            slot: CoverSlot::default(),
            cover_worker: None,
            clock: PanelClock::new(Instant::now()),
            hardware: HardwareState::default(),
        }
    }

    pub fn run<O: Outbox<SystemEvent>>(
        mut self,
        commands: &Receiver<SystemCmd>,
        outbox: &O,
    ) {
        autoreleasepool(|_| self.publish(Instant::now()));
        let arrivals = self.spawn_cover_worker();
        let (bell, rings) = bounded(1);
        match HardwareWatch::new(bell) {
            Ok(mut watch) => {
                autoreleasepool(|_| self.poll(&mut watch, outbox));
                self.serve(Session {
                    commands,
                    hardware: Some(HardwarePair {
                        watch: &mut watch,
                        rings: &rings,
                    }),
                    arrivals: arrivals.as_ref(),
                    outbox,
                });
            }
            Err(_) => self.serve(Session {
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

    fn serve<O: Outbox<SystemEvent>>(&mut self, mut session: Session<'_, O>) {
        loop {
            if let Flow::Stop = self.tick(&mut session) {
                return;
            }
        }
    }

    fn tick<O: Outbox<SystemEvent>>(&mut self, session: &mut Session<'_, O>) -> Flow {
        let mut select = Select::new();
        let command_case = select.recv(session.commands);
        let ring = session
            .hardware
            .as_ref()
            .map(|hardware| select.recv(hardware.rings));
        let arrival = session.arrivals.map(|arrivals| select.recv(arrivals));
        let selected = select.select();
        let index = selected.index();
        if index == command_case {
            self.on_command(selected, session.commands)
        } else if Some(index) == ring {
            self.on_ring(selected, session)
        } else if Some(index) == arrival {
            self.on_arrival(selected, session)
        } else {
            Flow::Stop
        }
    }

    fn on_command(
        &mut self,
        selected: SelectedOperation<'_>,
        commands: &Receiver<SystemCmd>,
    ) -> Flow {
        selected.recv(commands).map_or(Flow::Stop, |first| {
            self.drain_commands(first, commands);
            Flow::Continue
        })
    }

    fn on_ring<O: Outbox<SystemEvent>>(
        &mut self,
        selected: SelectedOperation<'_>,
        session: &mut Session<'_, O>,
    ) -> Flow {
        let Some(hardware) = &mut session.hardware else {
            return Flow::Stop;
        };
        match selected.recv(hardware.rings) {
            Ok(()) => autoreleasepool(|_| self.poll(hardware.watch, session.outbox)),
            Err(_) => Flow::Stop,
        }
    }

    fn on_arrival<O: Outbox<SystemEvent>>(
        &mut self,
        selected: SelectedOperation<'_>,
        session: &Session<'_, O>,
    ) -> Flow {
        let Some(arrivals) = session.arrivals else {
            return Flow::Stop;
        };
        selected.recv(arrivals).map_or(Flow::Stop, |bytes| {
            autoreleasepool(|_| self.arrived(bytes));
            Flow::Continue
        })
    }

    fn drain_commands(&mut self, first: SystemCmd, commands: &Receiver<SystemCmd>) {
        let batch = iter::once(first).chain(commands.try_iter()).collect();
        autoreleasepool(|_| {
            coalesced(batch).into_iter().for_each(|command| {
                self.perform(command, Instant::now());
            });
        });
    }

    fn perform(&mut self, command: SystemCmd, now: Instant) {
        match command {
            SystemCmd::NowPlaying(now_playing) => self.show(now_playing, now),
            SystemCmd::PlaybackState(playback) => {
                self.clock = self.clock.with_playback(playback, now);
                self.publish(now);
            }
            SystemCmd::PlaybackPosition(at) => {
                self.clock = self.clock.seek(at, now);
                self.publish(now);
            }
            SystemCmd::Volume(volume) => {
                if let Written::Landed = write_volume(current_default_device(), volume)
                {
                    self.hardware.note_written(volume);
                }
            }
        }
    }

    fn show(&mut self, now_playing: NowPlaying, now: Instant) {
        let track = match &now_playing {
            NowPlaying::Track { path, .. } => Some(path.clone()),
            NowPlaying::Cleared => None,
        };
        let Ok(effect) = self.slot.update(CoverMessage::Showing(track));
        self.apply_cover_effect(effect);
        self.showing = now_playing;
        self.clock = self.clock.seek(Duration::ZERO, now);
        self.publish(now);
    }

    fn arrived(&mut self, bytes: CoverBytes) {
        let Ok(effect) = self.slot.update(CoverMessage::Arrived(bytes));
        self.apply_cover_effect(effect);
        self.publish(Instant::now());
    }

    fn apply_cover_effect(&mut self, effect: CoverEffect) {
        match effect {
            CoverEffect::Nothing => {}
            CoverEffect::Clear | CoverEffect::ShowNone => self.artwork = None,
            CoverEffect::CoverWanted(track) => {
                self.artwork = None;
                if let Some(worker) = &self.cover_worker {
                    worker.want(track);
                }
            }
            CoverEffect::Show(bytes) => self.artwork = Some(Cover::from_bytes(&bytes)),
        }
    }

    fn publish(&self, now: Instant) {
        let panel = Panel {
            showing: &self.showing,
            clock: self.clock,
            artwork: self.artwork.as_ref().and_then(Cover::artwork),
        };
        publish(panel, now);
    }

    fn poll<O: Outbox<SystemEvent>>(
        &mut self,
        watch: &mut HardwareWatch,
        outbox: &O,
    ) -> Flow {
        let current = current_default_device();
        let read = HardwareRead {
            tracked_device: watch.tracked_device(),
            current_device: current,
            volume: read_volume(current),
        };
        let Ok(effect) = self.hardware.update(read);
        if let Some(device) = effect.rebind {
            let _ = watch.rebind_to(device);
        }
        deliver(effect.facts, outbox)
    }
}

fn deliver<O: Outbox<SystemEvent>>(facts: Vec<SystemEvent>, outbox: &O) -> Flow {
    for fact in facts {
        match outbox.send(fact) {
            Delivery::Sent | Delivery::Congested => {}
            Delivery::Closed => return Flow::Stop,
        }
    }
    Flow::Continue
}

fn coalesced(commands: Vec<SystemCmd>) -> Vec<SystemCmd> {
    let last_volume = commands.iter().rev().find_map(|command| match command {
        SystemCmd::Volume(volume) => Some(*volume),
        SystemCmd::NowPlaying(_)
        | SystemCmd::PlaybackState(_)
        | SystemCmd::PlaybackPosition(_) => None,
    });
    commands
        .into_iter()
        .filter(|command| !matches!(command, SystemCmd::Volume(_)))
        .chain(last_volume.map(SystemCmd::Volume))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use kernel::{
        Bounded,
        Delivery,
        Outbox,
        Percent,
        Playback,
        SystemCmd,
        SystemEvent,
        update::Machine,
    };
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::system_loop::{
        Flow,
        HardwareEffect,
        HardwareRead,
        HardwareState,
        RebindDecision,
        coalesced,
        deliver,
        rebind_decision,
    };

    fn volume(value: u8) -> SystemCmd {
        SystemCmd::Volume(Percent::clamped(value))
    }

    #[rstest]
    #[case::three_volumes(
        vec![volume(10), volume(20), volume(30)],
        vec![volume(30)]
    )]
    #[case::volumes_among_other_commands(
        vec![
            SystemCmd::PlaybackState(Playback::Playing),
            volume(10),
            SystemCmd::PlaybackState(Playback::Paused),
            volume(20),
        ],
        vec![
            SystemCmd::PlaybackState(Playback::Playing),
            SystemCmd::PlaybackState(Playback::Paused),
            volume(20),
        ]
    )]
    #[case::no_volume_at_all(
        vec![SystemCmd::PlaybackState(Playback::Playing)],
        vec![SystemCmd::PlaybackState(Playback::Playing)]
    )]
    fn coalesced_keeps_the_last_volume_last_and_the_rest_in_order(
        #[case] commands: Vec<SystemCmd>,
        #[case] applied: Vec<SystemCmd>,
    ) {
        assert_eq!(coalesced(commands), applied);
    }

    fn read(
        devices: (AudioObjectID, AudioObjectID),
        volume: Option<u8>,
    ) -> HardwareRead {
        let (tracked_device, current_device) = devices;
        HardwareRead {
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
                facts: vec![SystemEvent::OutputRouteChanged],
            },
            HardwareEffect::default(),
        ]
    )]
    #[case::the_first_volume_poll_is_always_reported(
        vec![read((1, 1), Some(30)), read((1, 1), Some(30))],
        vec![
            HardwareEffect {
                rebind: None,
                facts: vec![SystemEvent::Volume(Percent::clamped(30))],
            },
            HardwareEffect::default(),
        ]
    )]
    fn hardware_state_transitions_as_a_table(
        #[case] reads: Vec<HardwareRead>,
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
        sent: RefCell<Vec<SystemEvent>>,
        delivery: Delivery,
    }

    impl Outbox<SystemEvent> for FakeOutbox {
        fn send(&self, fact: SystemEvent) -> Delivery {
            self.sent.borrow_mut().push(fact);
            self.delivery
        }
    }

    #[rstest]
    #[case::sent(Delivery::Sent, Flow::Continue, 2)]
    #[case::congested(Delivery::Congested, Flow::Continue, 2)]
    #[case::closed(Delivery::Closed, Flow::Stop, 1)]
    fn deliver_stops_only_on_a_closed_outbox(
        #[case] delivery: Delivery,
        #[case] flow: Flow,
        #[case] delivered: usize,
    ) {
        let facts = vec![
            SystemEvent::OutputRouteChanged,
            SystemEvent::OutputRouteChanged,
        ];
        let outbox = FakeOutbox {
            sent: RefCell::new(Vec::new()),
            delivery,
        };

        let observed = deliver(facts, &outbox);

        assert_eq!(observed, flow);
        assert_eq!(outbox.sent.borrow().len(), delivered);
    }
}
