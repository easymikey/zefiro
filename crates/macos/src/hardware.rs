#![forbid(unsafe_code)]

use kernel::{
    Cmd,
    MacosEvent,
    Percent,
    update::{Machine, Unhandled},
};
use objc2_core_audio::AudioObjectID;

use crate::driver::MacosEffect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardwarePoll {
    pub(crate) tracked_device: AudioObjectID,
    pub(crate) current_device: AudioObjectID,
    pub(crate) volume: Option<Percent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HardwareMessage {
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
        if self.pending == Some(polled) {
            self.pending = None;
            self.last_reported = Some(polled);
            return None;
        }
        if self.last_reported == Some(polled) {
            return None;
        }
        self.last_reported = Some(polled);
        Some(MacosEvent::Volume(polled))
    }

    fn polled(&mut self, poll: HardwarePoll) -> Cmd<MacosEffect, MacosEvent> {
        let volume = poll.volume.and_then(|polled| self.heard_volume(polled));
        let route = self
            .device
            .is_some_and(|previous| previous != poll.current_device)
            .then_some(MacosEvent::OutputRouteChanged);
        let rebind = (poll.tracked_device != poll.current_device)
            .then_some(MacosEffect::Rebind(poll.current_device));
        self.device = Some(poll.current_device);
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
        match message {
            HardwareMessage::Polled(poll) => Ok(self.polled(poll)),
            HardwareMessage::VolumeSet(volume) => {
                self.pending = Some(volume);
                Ok(Cmd::none())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, Cmd, MacosEvent, Percent, update::Machine};
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::{
        driver::MacosEffect,
        hardware::{Hardware, HardwareMessage, HardwarePoll},
    };

    fn percent(level: u8) -> Percent {
        Percent::clamped(level)
    }

    fn poll(
        devices: (AudioObjectID, AudioObjectID),
        volume: Option<u8>,
    ) -> HardwareMessage {
        let (tracked_device, current_device) = devices;
        HardwareMessage::Polled(HardwarePoll {
            tracked_device,
            current_device,
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

    #[rstest]
    #[case::the_write_is_held(
        Hardware { pending: None, last_reported: Some(percent(30)), device: Some(1) },
        vec![HardwareMessage::VolumeSet(percent(40))],
        vec![Cmd::none()]
    )]
    #[case::the_echo(after_our_write(), vec![poll((1, 1), Some(40))], vec![Cmd::none()])]
    #[case::the_same_value_again_afterwards(
        after_our_write(),
        vec![poll((1, 1), Some(40)), poll((1, 1), Some(40))],
        vec![Cmd::none(), Cmd::none()]
    )]
    #[case::a_write_that_lands_slowly(
        after_our_write(),
        vec![poll((1, 1), Some(30)), poll((1, 1), Some(30)), poll((1, 1), Some(40))],
        vec![Cmd::none(), Cmd::none(), Cmd::none()]
    )]
    #[case::someone_else_moves_the_volume(
        after_our_write(),
        vec![poll((1, 1), Some(40)), poll((1, 1), Some(55))],
        vec![Cmd::none(), reported(55)]
    )]
    #[case::a_volume_only_event_never_rebinds(
        Hardware::default(),
        vec![poll((1, 1), None)],
        vec![Cmd::none()]
    )]
    #[case::a_changed_output_route_is_reported_once(
        Hardware::default(),
        vec![poll((1, 1), None), poll((1, 2), None), poll((2, 2), None)],
        vec![Cmd::none(), rerouted(2), Cmd::none()]
    )]
    #[case::back_to_the_first_output_is_a_change_too(
        Hardware::default(),
        vec![poll((1, 1), None), poll((1, 2), None), poll((2, 1), None)],
        vec![Cmd::none(), rerouted(2), rerouted(1)]
    )]
    #[case::the_first_volume_poll_is_always_reported(
        Hardware::default(),
        vec![poll((1, 1), Some(30)), poll((1, 1), Some(30))],
        vec![reported(30), Cmd::none()]
    )]
    #[case::volume_then_route_in_one_poll(
        Hardware::default(),
        vec![poll((1, 1), Some(30)), poll((1, 2), Some(50))],
        vec![
            reported(30),
            Cmd::effect(MacosEffect::Rebind(2))
                .then(reported(50))
                .then(Cmd::message(MacosEvent::OutputRouteChanged)),
        ]
    )]
    fn the_hardware_reports_only_what_changed(
        #[case] mut hardware: Hardware,
        #[case] messages: Vec<HardwareMessage>,
        #[case] cmds: Vec<Cmd<MacosEffect, MacosEvent>>,
    ) {
        let observed: Vec<Cmd<MacosEffect, MacosEvent>> = messages
            .into_iter()
            .map(|message| hardware.transition(message))
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(observed, cmds);
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
