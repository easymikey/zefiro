use std::time::Duration;

use crate::{
    cmd::{AudioCmd, Cmd, Effect, MacosCmd},
    domain::{
        cue::Cue,
        direction::Direction,
        index::PresetIndex,
        percent::Percent,
        player::AbLoop,
        revision::Revision,
        sleep::SleepTimer,
        time::Moment,
        transport::{OutputStatus, Transport},
    },
    message::Timer,
    update::machine::{Machine, Unhandled, replace},
};

#[derive(Debug, Clone)]
pub enum TransportMessage {
    StepVolume(Direction),
    SetVolume(Percent),
    StepSpeed(Direction),
    CycleSleep {
        sleep_timer: Option<SleepTimer>,
        revision: Revision,
    },
    AbMark(Option<Duration>),
    SleepFired,
}

impl Machine for Transport {
    type Message = TransportMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: TransportMessage) -> Result<Cmd, Unhandled> {
        match message {
            TransportMessage::StepVolume(direction) => {
                let volume = self.volume.step(direction);
                replace(&mut self.volume, volume)?;
                Ok(Effect::Macos(MacosCmd::SetVolume(self.volume)).into())
            }
            TransportMessage::SetVolume(volume) => {
                replace(&mut self.volume, volume)?;
                Ok(Cue::VolumeChanged.into())
            }
            TransportMessage::StepSpeed(direction) => {
                let speed = self.speed.step(direction);
                replace(&mut self.speed, speed)?;
                Ok(Cmd::from_iter([
                    Effect::Audio(AudioCmd::SetSpeed(self.speed)),
                    Effect::Macos(MacosCmd::SetSpeed(self.speed)),
                ]))
            }
            TransportMessage::CycleSleep {
                sleep_timer,
                revision,
            } => {
                replace(&mut self.sleep_timer, sleep_timer)?;
                Ok(self.sleep_timer.map_or(Cmd::none(), |timer| {
                    Effect::After {
                        delay: timer.delay,
                        timer: Timer::Sleep(revision),
                    }
                    .into()
                }))
            }
            TransportMessage::AbMark(None) => Err(Unhandled),
            TransportMessage::AbMark(Some(position)) => {
                let ab_loop = AbLoop::mark(self.ab_loop, position);
                replace(&mut self.ab_loop, ab_loop).map(|()| Cmd::none())
            }
            TransportMessage::SleepFired => {
                replace(&mut self.sleep_timer, None).map(|()| Cmd::none())
            }
        }
    }
}

impl Transport {
    pub(crate) fn output_ready(&mut self) {
        self.output_status = OutputStatus::Ready;
    }

    pub(crate) fn track_changed(&mut self) {
        self.ab_loop = None;
    }
}

pub(crate) fn next_sleep(
    current: Option<SleepTimer>,
    sleep_presets: &[Duration],
    now: Moment,
) -> Option<SleepTimer> {
    let position = current.map_or(0, |timer| timer.preset_index.get() + 1);
    sleep_presets.get(position).map(|&delay| SleepTimer {
        preset_index: PresetIndex::new(position),
        delay,
        deadline_at: Moment::new(now.since_epoch() + delay),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect, MacosCmd},
        domain::{
            bounded::Bounded,
            direction::Direction,
            index::PresetIndex,
            percent::Percent,
            player::AbLoop,
            revision::Revision,
            sleep::SleepTimer,
            sleep_presets::SleepPresets,
            speed::Speed,
            time::Moment,
            transport::{OutputStatus, Transport},
        },
        update::{
            machine::{Machine, Unhandled},
            transport::{TransportMessage, next_sleep},
        },
    };

    const NOW: Duration = Duration::from_secs(1_000);

    type State = (
        Percent,
        Speed,
        Option<SleepTimer>,
        Option<AbLoop>,
        OutputStatus,
    );

    fn state(transport: &Transport) -> State {
        (
            transport.volume,
            transport.speed,
            transport.sleep_timer,
            transport.ab_loop,
            transport.output_status,
        )
    }

    fn revision() -> Revision {
        Revision::default().next()
    }

    fn at_volume(percent: u8) -> Transport {
        Transport {
            volume: Percent::clamped(percent),
            ..Transport::default()
        }
    }

    fn at_speed(rate: f32) -> Transport {
        Transport {
            speed: Speed::clamped(rate),
            ..Transport::default()
        }
    }

    fn sleeping(preset: Option<(usize, u64)>) -> Transport {
        Transport {
            sleep_timer: preset.map(|(position, minutes)| timer(position, minutes)),
            ..Transport::default()
        }
    }

    fn looping(ab_loop: Option<AbLoop>) -> Transport {
        Transport {
            ab_loop,
            ..Transport::default()
        }
    }

    fn timer(preset: usize, minutes: u64) -> SleepTimer {
        let delay = Duration::from_mins(minutes);
        SleepTimer {
            preset_index: PresetIndex::new(preset),
            delay,
            deadline_at: Moment::new(NOW + delay),
        }
    }

    fn cycle_sleep(sleep_timer: Option<SleepTimer>) -> TransportMessage {
        TransportMessage::CycleSleep {
            sleep_timer,
            revision: revision(),
        }
    }

    fn system_volume(percent: u8) -> Cmd {
        Effect::Macos(MacosCmd::SetVolume(Percent::clamped(percent))).into()
    }

    fn mark(seconds: u64) -> TransportMessage {
        TransportMessage::AbMark(Some(Duration::from_secs(seconds)))
    }

    fn start_marked(seconds: u64) -> Option<AbLoop> {
        Some(AbLoop::StartMarked(Duration::from_secs(seconds)))
    }

    #[rstest]
    #[case::step_volume_up(
        at_volume(50),
        TransportMessage::StepVolume(Direction::Next),
        (at_volume(55), system_volume(55))
    )]
    #[case::step_volume_down(
        at_volume(50),
        TransportMessage::StepVolume(Direction::Previous),
        (at_volume(45), system_volume(45))
    )]
    #[case::step_volume_down_saturates_at_the_floor(
        at_volume(3),
        TransportMessage::StepVolume(Direction::Previous),
        (at_volume(0), system_volume(0))
    )]
    #[case::step_volume_up_saturates_at_the_ceiling(
        at_volume(98),
        TransportMessage::StepVolume(Direction::Next),
        (at_volume(100), system_volume(100))
    )]
    #[case::sleep_fired_clears_the_timer(
        sleeping(Some((1, 30))),
        TransportMessage::SleepFired,
        (sleeping(None), Cmd::none())
    )]
    fn a_transport_message_updates_the_transport(
        #[case] mut transport: Transport,
        #[case] message: TransportMessage,
        #[case] expected: (Transport, Cmd),
    ) {
        let (after, cmd) = expected;

        assert_eq!(transport.transition(message), Ok(cmd));
        assert_eq!(state(&transport), state(&after));
    }

    #[rstest]
    #[case::set_volume_to_the_current_level(
        at_volume(50),
        TransportMessage::SetVolume(Percent::clamped(50))
    )]
    #[case::ab_mark_without_a_position(
        looping(start_marked(10)),
        TransportMessage::AbMark(None)
    )]
    #[case::step_volume_at_the_ceiling(
        at_volume(100),
        TransportMessage::StepVolume(Direction::Next)
    )]
    #[case::step_volume_at_the_floor(
        at_volume(0),
        TransportMessage::StepVolume(Direction::Previous)
    )]
    #[case::step_speed_at_the_floor(
        at_speed(0.25),
        TransportMessage::StepSpeed(Direction::Previous)
    )]
    #[case::cycle_sleep_to_no_timer_without_a_timer(sleeping(None), cycle_sleep(None))]
    #[case::cycle_sleep_to_the_current_timer(
        sleeping(Some((1, 30))),
        cycle_sleep(Some(timer(1, 30)))
    )]
    #[case::ab_mark_before_the_a_point(looping(start_marked(10)), mark(5))]
    #[case::sleep_fired_without_a_timer(sleeping(None), TransportMessage::SleepFired)]
    fn a_transport_message_that_changes_nothing_is_refused(
        #[case] mut transport: Transport,
        #[case] message: TransportMessage,
    ) {
        let before = state(&transport);

        assert_eq!(transport.transition(message), Err(Unhandled));
        assert_eq!(state(&transport), before);
    }

    #[rstest]
    #[case::stays_off_without_presets(None, SleepPresets::from_minutes(&[]).unwrap(), None)]
    fn next_sleep_walks_the_presets(
        #[case] current: Option<SleepTimer>,
        #[case] presets: SleepPresets,
        #[case] expected: Option<SleepTimer>,
    ) {
        assert_eq!(
            next_sleep(current, presets.as_slice(), Moment::new(NOW)),
            expected
        );
    }
}
