#![forbid(unsafe_code)]

use std::{
    iter,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, Sender, unbounded};
use kernel::{
    AudioEvent,
    Message,
    NowPlaying,
    Percent,
    SystemCmd,
    update::{Machine, Never, Rejected},
};
use objc2::rc::autoreleasepool;
use objc2_core_audio::AudioObjectID;

use crate::{
    audio_hardware::{HardwareSignal, HardwareWatch, current_default_device},
    clock::PanelClock,
    cover::{Cover, CoverReader},
    echo::{VolumeEcho, VolumeEffect, VolumeMessage},
    now_playing::{Panel, publish},
    output::{DefaultOutput, OutputEffect, OutputPolled, default_output_name},
    volume::{read_volume, write_volume},
};

#[derive(Debug)]
pub struct SystemLoop {
    read_cover: CoverReader,
    showing: NowPlaying,
    cover: Option<Cover>,
    clock: PanelClock,
    hardware: HardwareState,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HardwareRead {
    pub(crate) tracked_device: AudioObjectID,
    pub(crate) current_device: AudioObjectID,
    pub(crate) volume: Option<Percent>,
    pub(crate) output_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HardwareEffect {
    pub(crate) rebind: Option<AudioObjectID>,
    pub(crate) messages: Vec<Message>,
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
        let mut messages = Vec::new();
        let echo = match read.volume {
            Some(volume) => {
                let Ok((echo, effect)) =
                    self.echo.transition(VolumeMessage::Polled(volume));
                if let VolumeEffect::Report(reported) = effect {
                    messages.push(Message::SystemVolume(reported));
                }
                echo
            }
            None => self.echo,
        };
        let Ok((output, effect)) =
            self.output.transition(OutputPolled(read.output_name));
        if let OutputEffect::Changed = effect {
            messages.push(Message::Audio(AudioEvent::OutputRouteChanged));
        }
        let rebind = match rebind_decision(read.tracked_device, read.current_device) {
            RebindDecision::Unchanged => None,
            RebindDecision::Changed(device) => Some(device),
        };
        Ok((Self { echo, output }, HardwareEffect { rebind, messages }))
    }
}

impl SystemLoop {
    #[must_use]
    pub fn new(read_cover: CoverReader) -> Self {
        Self {
            read_cover,
            showing: NowPlaying::Cleared,
            cover: None,
            clock: PanelClock::new(Instant::now()),
            hardware: HardwareState::default(),
        }
    }

    pub fn run(mut self, commands: &Receiver<SystemCmd>, mailbox: &Sender<Message>) {
        autoreleasepool(|_| self.publish(Instant::now()));
        let (hardware_sender, hardware_events) = unbounded();
        let mut watch = HardwareWatch::new(hardware_sender);
        self.poll(&mut watch, mailbox);
        loop {
            let mut select = Select::new();
            let command_case = select.recv(commands);
            let hardware_case = select.recv(&hardware_events);
            let selected = select.select();
            match selected.index() {
                index if index == command_case => match selected.recv(commands) {
                    Ok(first) => {
                        let batch =
                            iter::once(first).chain(commands.try_iter()).collect();
                        autoreleasepool(|_| {
                            coalesced(batch).into_iter().for_each(|command| {
                                self.perform(command, Instant::now());
                            });
                        });
                    }
                    Err(_) => return,
                },
                index if index == hardware_case => {
                    match selected.recv(&hardware_events) {
                        Ok(HardwareSignal::Changed) => {
                            hardware_events.try_iter().for_each(drop);
                            autoreleasepool(|_| self.poll(&mut watch, mailbox));
                        }
                        Err(_) => return,
                    }
                }
                _ => return,
            }
        }
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
                write_volume(volume);
                self.hardware.note_written(volume);
            }
        }
    }

    fn show(&mut self, now_playing: NowPlaying, now: Instant) {
        let track = match &now_playing {
            NowPlaying::Track { path, .. } => Some(path.as_path()),
            NowPlaying::Cleared => None,
        };
        if self.cover.as_ref().map(Cover::track) != track {
            self.cover = track.map(|path| Cover::new(path, self.read_cover));
        }
        self.showing = now_playing;
        self.clock = self.clock.seek(Duration::ZERO, now);
        self.publish(now);
    }

    fn publish(&self, now: Instant) {
        let panel = Panel {
            showing: &self.showing,
            clock: self.clock,
            artwork: self.cover.as_ref().and_then(Cover::artwork),
        };
        publish(panel, now);
    }

    fn poll(&mut self, watch: &mut HardwareWatch, mailbox: &Sender<Message>) {
        let read = HardwareRead {
            tracked_device: watch.tracked_device(),
            current_device: current_default_device(),
            volume: read_volume(),
            output_name: default_output_name(),
        };
        let Ok(effect) = self.hardware.update(read);
        if let Some(device) = effect.rebind {
            watch.rebind_to(device);
        }
        effect.messages.into_iter().for_each(|message| {
            let _ = mailbox.send(message);
        });
    }
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
    use kernel::{
        AudioEvent,
        Bounded,
        Message,
        Percent,
        Playback,
        SystemCmd,
        update::Machine,
    };
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::system_loop::{
        HardwareEffect,
        HardwareRead,
        HardwareState,
        RebindDecision,
        coalesced,
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
        output: Option<&str>,
    ) -> HardwareRead {
        let (tracked_device, current_device) = devices;
        HardwareRead {
            tracked_device,
            current_device,
            volume: volume.map(Percent::clamped),
            output_name: output.map(str::to_owned),
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
        vec![read((1, 1), None, None)],
        vec![HardwareEffect::default()]
    )]
    #[case::rebind_fires_only_once_the_default_device_changes(
        vec![read((1, 1), None, None), read((1, 2), None, None)],
        vec![
            HardwareEffect::default(),
            HardwareEffect { rebind: Some(2), messages: Vec::new() },
        ]
    )]
    #[case::a_changed_output_route_is_reported_once(
        vec![
            read((1, 1), None, Some("Speakers")),
            read((1, 1), None, Some("Headphones")),
            read((1, 1), None, Some("Headphones")),
        ],
        vec![
            HardwareEffect::default(),
            HardwareEffect {
                rebind: None,
                messages: vec![Message::Audio(AudioEvent::OutputRouteChanged)],
            },
            HardwareEffect::default(),
        ]
    )]
    #[case::the_first_volume_poll_is_always_reported(
        vec![read((1, 1), Some(30), None), read((1, 1), Some(30), None)],
        vec![
            HardwareEffect {
                rebind: None,
                messages: vec![Message::SystemVolume(Percent::clamped(30))],
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
}
