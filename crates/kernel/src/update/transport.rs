use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, MacosCmd},
    domain::{AbLoop, Percent, Revision, SleepPresets, SleepTimer, Transport},
    message::Timer,
};

#[derive(Debug, Clone)]
pub enum TransportMessage {
    StepVolume {
        steps: i8,
    },
    SetVolume(Percent),
    StepSpeed {
        steps: i8,
    },
    CycleSleep {
        presets: SleepPresets,
        revision: Revision,
    },
    AbMark {
        position: Option<Duration>,
    },
}

impl Transport {
    pub fn apply(&mut self, message: TransportMessage) -> Cmd {
        match message {
            TransportMessage::StepVolume { steps } => {
                self.volume = self.volume.step(steps);
                Effect::Macos(MacosCmd::Volume(self.volume)).into()
            }
            TransportMessage::SetVolume(volume) if volume == self.volume => Cmd::None,
            TransportMessage::SetVolume(volume) => {
                self.volume = volume;
                Cue::VolumeChanged.into()
            }
            TransportMessage::StepSpeed { steps } if steps > 0 => {
                self.speed = self.speed.step_up();
                Effect::Audio(AudioCmd::SetSpeed(self.speed)).into()
            }
            TransportMessage::StepSpeed { .. } => {
                self.speed = self.speed.step_down();
                Effect::Audio(AudioCmd::SetSpeed(self.speed)).into()
            }
            TransportMessage::CycleSleep { presets, revision } => {
                self.sleep = next_sleep(self.sleep, presets.as_slice());
                self.sleep.map_or(Cmd::None, |timer| {
                    Effect::After {
                        delay: timer.delay,
                        timer: Timer::Sleep(revision),
                    }
                    .into()
                })
            }
            TransportMessage::AbMark { position: None } => Cmd::None,
            TransportMessage::AbMark {
                position: Some(position),
            } => {
                self.ab_loop = AbLoop::mark(self.ab_loop, position);
                Cmd::None
            }
        }
    }
}

fn next_sleep(
    current: Option<SleepTimer>,
    sleep_presets: &[Duration],
) -> Option<SleepTimer> {
    let preset_index = current.map_or(0, |timer| timer.preset_index + 1);
    sleep_presets.get(preset_index).map(|&delay| SleepTimer {
        preset_index,
        delay,
    })
}
