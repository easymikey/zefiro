#![forbid(unsafe_code)]

use kernel::{
    cmd::Cmd,
    domain::percent::Percent,
    message::MacosEvent,
    update::machine::{Machine, Unhandled},
};
use objc2_core_audio::AudioObjectID;

use crate::effect::MacosEffect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwarePoll {
    pub(crate) tracked: AudioObjectID,
    pub(crate) current: AudioObjectID,
    pub(crate) volume: Option<Percent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareMessage {
    Polled(HardwarePoll),
    VolumeSet(Percent),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Hardware {
    pending: Option<Percent>,
    last_reported: Option<Percent>,
    device: Option<AudioObjectID>,
}

impl Hardware {
    fn heard_volume(&mut self, polled: Percent) -> Option<MacosEvent> {
        let echo = self
            .pending
            .is_some_and(|pending| pending.get().abs_diff(polled.get()) <= 1);
        if echo {
            self.pending = None;
            self.last_reported = Some(polled);
            return None;
        }
        if self.last_reported == Some(polled) {
            return None;
        }
        self.pending = None;
        self.last_reported = Some(polled);
        Some(MacosEvent::Volume(polled))
    }

    fn polled(&mut self, poll: HardwarePoll) -> Cmd<MacosEffect, MacosEvent> {
        let volume = poll.volume.and_then(|polled| self.heard_volume(polled));
        let route = self
            .device
            .is_some_and(|previous| previous != poll.current)
            .then_some(MacosEvent::OutputRouteChanged);
        let rebind =
            (poll.tracked != poll.current).then_some(MacosEffect::Rebind(poll.current));
        self.device = Some(poll.current);
        [volume, route]
            .into_iter()
            .flatten()
            .map(Cmd::message)
            .fold(rebind.into_iter().collect(), Cmd::then)
    }
}

impl Machine for Hardware {
    type Message = HardwareMessage;
    type Effect = Cmd<MacosEffect, MacosEvent>;

    fn transition(
        &mut self,
        message: HardwareMessage,
    ) -> Result<Self::Effect, Unhandled> {
        let mut next = *self;
        let cmd = match message {
            HardwareMessage::Polled(poll) => next.polled(poll),
            HardwareMessage::VolumeSet(volume) => {
                next.pending = Some(volume);
                Cmd::none()
            }
        };
        if next == *self && cmd == Cmd::none() {
            return Err(Unhandled);
        }
        *self = next;
        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use kernel::{
        cmd::Cmd,
        domain::{bounded::Bounded, percent::Percent},
        message::MacosEvent,
        update::machine::{Machine, Unhandled},
    };
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::{
        effect::MacosEffect,
        hardware::{Hardware, HardwareMessage, HardwarePoll},
    };

    fn percent(level: u8) -> Percent {
        Percent::clamped(level)
    }

    fn poll(
        devices: (AudioObjectID, AudioObjectID),
        volume: Option<u8>,
    ) -> HardwareMessage {
        let (tracked, current) = devices;
        HardwareMessage::Polled(HardwarePoll {
            tracked,
            current,
            volume: volume.map(Percent::clamped),
        })
    }

    fn reported(level: u8) -> Cmd<MacosEffect, MacosEvent> {
        Cmd::message(MacosEvent::Volume(percent(level)))
    }

    fn rerouted(device: AudioObjectID) -> Cmd<MacosEffect, MacosEvent> {
        Cmd::effect(MacosEffect::Rebind(device))
            .then(Cmd::message(MacosEvent::OutputRouteChanged))
    }

    fn after_our_write() -> Hardware {
        Hardware {
            pending: Some(percent(40)),
            last_reported: Some(percent(30)),
            device: Some(1),
        }
    }

    fn quiet() -> Result<Cmd<MacosEffect, MacosEvent>, Unhandled> {
        Ok(Cmd::none())
    }

    #[rstest]
    #[case::the_write_is_held(
        Hardware { pending: None, last_reported: Some(percent(30)), device: Some(1) },
        vec![HardwareMessage::VolumeSet(percent(40))],
        vec![quiet()]
    )]
    #[case::the_echo(after_our_write(), vec![poll((1, 1), Some(40))], vec![quiet()])]
    #[case::a_quantised_echo_then_its_repeat_is_refused(
        after_our_write(),
        vec![poll((1, 1), Some(39)), poll((1, 1), Some(39))],
        vec![quiet(), Err(Unhandled)]
    )]
    #[case::a_change_by_someone_else_drops_the_held_write(
        after_our_write(),
        vec![poll((1, 1), Some(55)), poll((1, 1), Some(40))],
        vec![Ok(reported(55)), Ok(reported(40))]
    )]
    #[case::the_same_value_again_afterwards_is_refused(
        after_our_write(),
        vec![poll((1, 1), Some(40)), poll((1, 1), Some(40))],
        vec![quiet(), Err(Unhandled)]
    )]
    #[case::a_write_that_lands_slowly_is_refused_until_it_lands(
        after_our_write(),
        vec![poll((1, 1), Some(30)), poll((1, 1), Some(30)), poll((1, 1), Some(40))],
        vec![Err(Unhandled), Err(Unhandled), quiet()]
    )]
    #[case::someone_else_moves_the_volume(
        after_our_write(),
        vec![poll((1, 1), Some(40)), poll((1, 1), Some(55))],
        vec![quiet(), Ok(reported(55))]
    )]
    #[case::a_volume_only_event_never_rebinds(
        Hardware::default(),
        vec![poll((1, 1), None)],
        vec![quiet()]
    )]
    #[case::a_changed_output_route_is_reported_once_and_its_repeat_refused(
        Hardware::default(),
        vec![poll((1, 1), None), poll((1, 2), None), poll((2, 2), None)],
        vec![quiet(), Ok(rerouted(2)), Err(Unhandled)]
    )]
    #[case::back_to_the_first_output_is_a_change_too(
        Hardware::default(),
        vec![poll((1, 1), None), poll((1, 2), None), poll((2, 1), None)],
        vec![quiet(), Ok(rerouted(2)), Ok(rerouted(1))]
    )]
    #[case::the_first_volume_poll_is_always_reported_and_its_repeat_refused(
        Hardware::default(),
        vec![poll((1, 1), Some(30)), poll((1, 1), Some(30))],
        vec![Ok(reported(30)), Err(Unhandled)]
    )]
    #[case::volume_then_route_in_one_poll(
        Hardware::default(),
        vec![poll((1, 1), Some(30)), poll((1, 2), Some(50))],
        vec![
            Ok(reported(30)),
            Ok(Cmd::effect(MacosEffect::Rebind(2))
                .then(reported(50))
                .then(Cmd::message(MacosEvent::OutputRouteChanged))),
        ]
    )]
    #[case::a_write_already_held_is_refused(
        after_our_write(),
        vec![HardwareMessage::VolumeSet(percent(40))],
        vec![Err(Unhandled)]
    )]
    fn the_hardware_reports_only_what_changed_and_refuses_the_rest(
        #[case] mut hardware: Hardware,
        #[case] messages: Vec<HardwareMessage>,
        #[case] cmds: Vec<Result<Cmd<MacosEffect, MacosEvent>, Unhandled>>,
    ) {
        assert_eq!(
            messages
                .into_iter()
                .map(|message| hardware.transition(message))
                .collect::<Vec<_>>(),
            cmds
        );
    }

    #[test]
    fn a_refused_poll_leaves_the_hardware_as_it_was() {
        let mut hardware = after_our_write();
        let before = hardware;
        assert_eq!(hardware.transition(poll((1, 1), Some(30))), Err(Unhandled));
        assert_eq!(hardware, before);
    }

    #[test]
    fn a_held_write_waits_for_its_echo() {
        let mut hardware = Hardware {
            pending: None,
            last_reported: Some(percent(30)),
            device: Some(1),
        };
        assert_eq!(
            hardware.transition(HardwareMessage::VolumeSet(percent(40))),
            Ok(Cmd::none())
        );
        assert_eq!(hardware, after_our_write());
    }
}
