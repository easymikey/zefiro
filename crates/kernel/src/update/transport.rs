use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, SystemCmd},
    domain::{AbLoop, Percent, Revision, SleepTimer, Transport},
    message::Timer,
    update::machine::{Machine, Never, Rejected},
};

#[derive(Debug, Clone)]
pub enum TransportMessage {
    NudgeVolume(i8),
    SetVolume(Percent),
    NudgeSpeed(i8),
    CycleSleep(Box<[Duration]>),
    AbMark { position: Option<Duration> },
}

impl Machine for Transport {
    type Message = TransportMessage;
    type Rejection = Never;
    type Effect = Cmd;

    fn transition(
        mut self,
        message: TransportMessage,
    ) -> Result<(Self, Cmd), Rejected<Self>> {
        let cmd = match message {
            TransportMessage::NudgeVolume(delta) => {
                self.volume = self.volume.nudge(delta);
                Cmd::Batch(vec![
                    Effect::Audio(AudioCmd::Volume(self.volume)),
                    Effect::System(SystemCmd::Volume(self.volume)),
                ])
            }
            TransportMessage::SetVolume(volume) if volume == self.volume => Cmd::None,
            TransportMessage::SetVolume(volume) => {
                self.volume = volume;
                Cue::VolumeChanged.into()
            }
            TransportMessage::NudgeSpeed(delta) if delta > 0 => {
                self.speed = self.speed.step_up();
                Effect::Audio(AudioCmd::SetSpeed(self.speed)).into()
            }
            TransportMessage::NudgeSpeed(_) => {
                self.speed = self.speed.step_down();
                Effect::Audio(AudioCmd::SetSpeed(self.speed)).into()
            }
            TransportMessage::CycleSleep(sleep_presets) => {
                self.sleep = next_sleep(self.sleep, &sleep_presets);
                self.sleep.map_or(Cmd::None, |timer| {
                    Effect::After {
                        delay: timer.delay,
                        message: Timer::Sleep(Revision::UNSTAMPED),
                    }
                    .into()
                })
            }
            TransportMessage::AbMark { position: None } => Cmd::None,
            TransportMessage::AbMark {
                position: Some(position),
            } => {
                self.ab = AbLoop::mark(self.ab, position);
                Cmd::None
            }
        };
        Ok((self, cmd))
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
