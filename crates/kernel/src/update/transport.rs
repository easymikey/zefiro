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
        sleep_presets::SleepPresets,
        time::Moment,
        transport::{Output, StreamError, Transport},
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
        now: Moment,
    },
    AbMark(Option<Duration>),
    OutputLost(StreamError),
    OutputReady,
    TrackChanged,
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
                Ok(Effect::Audio(AudioCmd::SetSpeed(self.speed)).into())
            }
            TransportMessage::CycleSleep {
                presets,
                revision,
                now,
            } => {
                let sleep = next_sleep(self.sleep, presets.as_slice(), now);
                replace(&mut self.sleep, sleep)?;
                Ok(self.sleep.map_or(Cmd::none(), |timer| {
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
                replace(&mut self.ab_loop, ab_loop)?;
                Ok(Cmd::none())
            }
            TransportMessage::OutputLost(error) => {
                self.output = Output::Lost(error);
                Ok(Cmd::none())
            }
            TransportMessage::OutputReady => {
                self.output = Output::Ready;
                Ok(Cmd::none())
            }
            TransportMessage::TrackChanged => {
                replace(&mut self.ab_loop, None)?;
                Ok(Cmd::none())
            }
            TransportMessage::SleepFired => {
                replace(&mut self.sleep, None)?;
                Ok(Cmd::none())
            }
        }
    }
}

fn replace<T: PartialEq>(field: &mut T, next: T) -> Result<(), Unhandled> {
    if *field == next {
        Err(Unhandled)
    } else {
        *field = next;
        Ok(())
    }
}

fn next_sleep(
    current: Option<SleepTimer>,
    sleep_presets: &[Duration],
    now: Moment,
) -> Option<SleepTimer> {
    let position = current.map_or(0, |timer| timer.preset_index.get() + 1);
    sleep_presets.get(position).map(|&delay| SleepTimer {
        preset_index: PresetIndex::new(position),
        delay,
        deadline: Moment::new(now.since_epoch() + delay),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, Effect, MacosCmd},
        domain::{
            bounded::Bounded,
            cue::Cue,
            direction::Direction,
            index::PresetIndex,
            percent::Percent,
            player::AbLoop,
            revision::Revision,
            sleep::SleepTimer,
            sleep_presets::SleepPresets,
            speed::Speed,
            time::Moment,
            transport::{Output, StreamError, Transport},
        },
        message::Timer,
        update::{
            machine::{Machine, Unhandled},
            transport::TransportMessage,
        },
    };

    const NOW: Duration = Duration::from_secs(1_000);

    type State = (Percent, Speed, Option<SleepTimer>, Option<AbLoop>, Output);

    fn state(transport: &Transport) -> State {
        (
            transport.volume,
            transport.speed,
            transport.sleep,
            transport.ab_loop,
            transport.output.clone(),
        )
    }

    fn revision() -> Revision {
        Revision::default().next()
    }

    fn output_lost() -> Transport {
        Transport {
            output: Output::Lost(StreamError::Backend),
            ..Transport::default()
        }
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
            sleep: preset.map(|(position, minutes)| timer(position, minutes)),
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
            deadline: Moment::new(NOW + delay),
        }
    }

    fn cycle_sleep() -> TransportMessage {
        cycle_sleep_with(SleepPresets::default())
    }

    fn cycle_sleep_with(presets: SleepPresets) -> TransportMessage {
        TransportMessage::CycleSleep {
            presets,
            revision: revision(),
            now: Moment::new(NOW),
        }
    }

    fn sleep_after(minutes: u64) -> Cmd {
        Effect::After {
            delay: Duration::from_mins(minutes),
            timer: Timer::Sleep(revision()),
        }
        .into()
    }

    fn system_volume(percent: u8) -> Cmd {
        Effect::Macos(MacosCmd::SetVolume(Percent::clamped(percent))).into()
    }

    fn audio_speed(rate: f32) -> Cmd {
        Effect::Audio(AudioCmd::SetSpeed(Speed::clamped(rate))).into()
    }

    fn mark(seconds: u64) -> TransportMessage {
        TransportMessage::AbMark(Some(Duration::from_secs(seconds)))
    }

    fn a_only(seconds: u64) -> Option<AbLoop> {
        Some(AbLoop::AOnly(Duration::from_secs(seconds)))
    }

    fn a_to_b(a: u64, b: u64) -> Option<AbLoop> {
        Some(AbLoop::Full {
            a: Duration::from_secs(a),
            b: Duration::from_secs(b),
        })
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
    #[case::set_volume_to_a_new_level(
        at_volume(50),
        TransportMessage::SetVolume(Percent::clamped(70)),
        (at_volume(70), Cue::VolumeChanged.into())
    )]
    #[case::step_speed_up(
        at_speed(1.0),
        TransportMessage::StepSpeed(Direction::Next),
        (at_speed(1.25), audio_speed(1.25))
    )]
    #[case::step_speed_down(
        at_speed(1.0),
        TransportMessage::StepSpeed(Direction::Previous),
        (at_speed(0.75), audio_speed(0.75))
    )]
    #[case::cycle_sleep_starts_at_the_first_preset(
        sleeping(None),
        cycle_sleep(),
        (sleeping(Some((0, 15))), sleep_after(15))
    )]
    #[case::cycle_sleep_moves_to_the_next_preset(
        sleeping(Some((0, 15))),
        cycle_sleep(),
        (sleeping(Some((1, 30))), sleep_after(30))
    )]
    #[case::cycle_sleep_wraps_past_the_last_preset_to_off(
        sleeping(Some((2, 60))),
        cycle_sleep(),
        (sleeping(None), Cmd::none())
    )]
    #[case::ab_mark_sets_a(
        looping(None),
        mark(10),
        (looping(a_only(10)), Cmd::none())
    )]
    #[case::ab_mark_sets_b_after_a(
        looping(a_only(10)),
        mark(20),
        (looping(a_to_b(10, 20)), Cmd::none())
    )]
    #[case::output_lost_records_the_error(
        Transport::default(),
        TransportMessage::OutputLost(StreamError::Backend),
        (output_lost(), Cmd::none())
    )]
    #[case::output_lost_with_the_same_error_records_it_again(
        output_lost(),
        TransportMessage::OutputLost(StreamError::Backend),
        (output_lost(), Cmd::none())
    )]
    #[case::output_ready_while_ready_stays_ready(
        Transport::default(),
        TransportMessage::OutputReady,
        (Transport::default(), Cmd::none())
    )]
    #[case::output_ready_clears_the_loss(
        output_lost(),
        TransportMessage::OutputReady,
        (Transport::default(), Cmd::none())
    )]
    #[case::track_changed_clears_the_loop(
        looping(a_to_b(10, 20)),
        TransportMessage::TrackChanged,
        (looping(None), Cmd::none())
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
        looping(a_only(10)),
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
    #[case::step_speed_at_the_ceiling(
        at_speed(4.0),
        TransportMessage::StepSpeed(Direction::Next)
    )]
    #[case::step_speed_at_the_floor(
        at_speed(0.25),
        TransportMessage::StepSpeed(Direction::Previous)
    )]
    #[case::cycle_sleep_without_presets_and_without_a_timer(
        sleeping(None),
        cycle_sleep_with(SleepPresets::from_minutes(&[]).unwrap())
    )]
    #[case::ab_mark_at_the_a_point(looping(a_only(10)), mark(10))]
    #[case::ab_mark_before_the_a_point(looping(a_only(10)), mark(5))]
    #[case::track_changed_without_a_loop(looping(None), TransportMessage::TrackChanged)]
    #[case::sleep_fired_without_a_timer(sleeping(None), TransportMessage::SleepFired)]
    fn a_transport_message_that_changes_nothing_is_refused(
        #[case] mut transport: Transport,
        #[case] message: TransportMessage,
    ) {
        let before = state(&transport);

        assert_eq!(transport.transition(message), Err(Unhandled));
        assert_eq!(state(&transport), before);
    }
}
