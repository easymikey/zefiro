use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, MacosCmd},
    domain::{
        AbLoop,
        Direction,
        Percent,
        PresetIndex,
        Revision,
        SleepPresets,
        SleepTimer,
        Transport,
    },
    message::Timer,
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone)]
pub enum TransportMessage {
    StepVolume(Direction),
    SetVolume(Percent),
    StepSpeed(Direction),
    CycleSleep {
        presets: SleepPresets,
        revision: Revision,
    },
    AbMark(Option<Duration>),
}

impl Machine for Transport {
    type Message = TransportMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: TransportMessage) -> Result<Cmd, Unhandled> {
        Ok(match message {
            TransportMessage::StepVolume(direction) => {
                self.volume = self.volume.step_by(direction);
                Effect::Macos(MacosCmd::SetVolume(self.volume)).into()
            }
            TransportMessage::SetVolume(volume) if volume == self.volume => Cmd::none(),
            TransportMessage::SetVolume(volume) => {
                self.volume = volume;
                Cue::VolumeChanged.into()
            }
            TransportMessage::StepSpeed(Direction::Next) => {
                self.speed = self.speed.step_up();
                Effect::Audio(AudioCmd::SetSpeed(self.speed)).into()
            }
            TransportMessage::StepSpeed(Direction::Previous) => {
                self.speed = self.speed.step_down();
                Effect::Audio(AudioCmd::SetSpeed(self.speed)).into()
            }
            TransportMessage::CycleSleep { presets, revision } => {
                self.sleep = next_sleep(self.sleep, presets.as_slice());
                self.sleep.map_or(Cmd::none(), |timer| {
                    Effect::After {
                        delay: timer.delay,
                        timer: Timer::Sleep(revision),
                    }
                    .into()
                })
            }
            TransportMessage::AbMark(None) => Cmd::none(),
            TransportMessage::AbMark(Some(position)) => {
                self.ab_loop = AbLoop::mark(self.ab_loop, position);
                Cmd::none()
            }
        })
    }
}

fn next_sleep(
    current: Option<SleepTimer>,
    sleep_presets: &[Duration],
) -> Option<SleepTimer> {
    let position = current.map_or(0, |timer| timer.preset_index.get() + 1);
    sleep_presets.get(position).map(|&delay| SleepTimer {
        preset_index: PresetIndex::new(position),
        delay,
    })
}
