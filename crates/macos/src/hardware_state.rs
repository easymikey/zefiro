#![forbid(unsafe_code)]

use kernel::{MacosEvent, Percent};
use objc2_core_audio::AudioObjectID;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct VolumeEcho {
    pending: Option<Percent>,
    last_reported: Option<Percent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeMessage {
    Written(Percent),
    Polled(Percent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeEffect {
    Nothing,
    Changed(Percent),
}

impl VolumeEcho {
    pub(crate) fn apply(&mut self, message: VolumeMessage) -> VolumeEffect {
        match message {
            VolumeMessage::Written(volume) => {
                self.pending = Some(volume);
                VolumeEffect::Nothing
            }
            VolumeMessage::Polled(volume) if self.pending == Some(volume) => {
                self.pending = None;
                self.last_reported = Some(volume);
                VolumeEffect::Nothing
            }
            VolumeMessage::Polled(volume) if self.last_reported == Some(volume) => {
                VolumeEffect::Nothing
            }
            VolumeMessage::Polled(volume) => {
                self.last_reported = Some(volume);
                VolumeEffect::Changed(volume)
            }
        }
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
    pub(crate) echo: VolumeEcho,
    device: Option<AudioObjectID>,
}

impl HardwareState {
    pub(crate) fn apply(&mut self, message: HardwareMessage) -> HardwareEffect {
        let volume = message.volume.and_then(|polled| {
            match self.echo.apply(VolumeMessage::Polled(polled)) {
                VolumeEffect::Changed(reported) => Some(MacosEvent::Volume(reported)),
                VolumeEffect::Nothing => None,
            }
        });
        let route = self
            .device
            .is_some_and(|previous| previous != message.current_device)
            .then_some(MacosEvent::OutputRouteChanged);
        let rebind = (message.tracked_device != message.current_device)
            .then_some(message.current_device);
        self.device = Some(message.current_device);
        HardwareEffect {
            rebind,
            events: [volume, route].into_iter().flatten().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, MacosEvent, Percent};
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::hardware_state::{
        HardwareEffect,
        HardwareMessage,
        HardwareState,
        VolumeEcho,
        VolumeEffect,
        VolumeMessage,
    };

    fn percent(level: u8) -> Percent {
        Percent::clamped(level)
    }

    #[rstest]
    #[case::the_echo(&[40], &[VolumeEffect::Nothing])]
    #[case::the_same_value_again_afterwards(
        &[40, 40],
        &[VolumeEffect::Nothing, VolumeEffect::Nothing]
    )]
    #[case::a_write_that_lands_slowly(
        &[30, 30, 40],
        &[VolumeEffect::Nothing, VolumeEffect::Nothing, VolumeEffect::Nothing]
    )]
    #[case::someone_else_moves_the_volume(
        &[40, 55],
        &[VolumeEffect::Nothing, VolumeEffect::Changed(Percent::clamped(55))]
    )]
    fn volume_echo_suppresses_our_own_write(
        #[case] polls: &[u8],
        #[case] reported: &[VolumeEffect],
    ) {
        let mut echo = VolumeEcho::default();
        assert_eq!(
            echo.apply(VolumeMessage::Polled(percent(30))),
            VolumeEffect::Changed(percent(30))
        );
        assert_eq!(
            echo.apply(VolumeMessage::Written(percent(40))),
            VolumeEffect::Nothing
        );
        let observed: Vec<VolumeEffect> = polls
            .iter()
            .map(|value| echo.apply(VolumeMessage::Polled(percent(*value))))
            .collect();
        assert_eq!(observed, reported);
    }

    fn poll(
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
    #[case::a_volume_only_event_never_rebinds(
        vec![poll((1, 1), None)],
        vec![HardwareEffect::default()]
    )]
    #[case::a_changed_output_route_is_reported_once(
        vec![poll((1, 1), None), poll((1, 2), None), poll((2, 2), None)],
        vec![
            HardwareEffect::default(),
            HardwareEffect {
                rebind: Some(2),
                events: vec![MacosEvent::OutputRouteChanged],
            },
            HardwareEffect::default(),
        ]
    )]
    #[case::back_to_the_first_output_is_a_change_too(
        vec![poll((1, 1), None), poll((1, 2), None), poll((2, 1), None)],
        vec![
            HardwareEffect::default(),
            HardwareEffect {
                rebind: Some(2),
                events: vec![MacosEvent::OutputRouteChanged],
            },
            HardwareEffect {
                rebind: Some(1),
                events: vec![MacosEvent::OutputRouteChanged],
            },
        ]
    )]
    #[case::the_first_volume_poll_is_always_reported(
        vec![poll((1, 1), Some(30)), poll((1, 1), Some(30))],
        vec![
            HardwareEffect {
                rebind: None,
                events: vec![MacosEvent::Volume(Percent::clamped(30))],
            },
            HardwareEffect::default(),
        ]
    )]
    #[case::volume_then_route_in_one_poll(
        vec![poll((1, 1), Some(30)), poll((1, 2), Some(50))],
        vec![
            HardwareEffect {
                rebind: None,
                events: vec![MacosEvent::Volume(Percent::clamped(30))],
            },
            HardwareEffect {
                rebind: Some(2),
                events: vec![
                    MacosEvent::Volume(Percent::clamped(50)),
                    MacosEvent::OutputRouteChanged,
                ],
            },
        ]
    )]
    fn a_hardware_poll_reports_only_what_changed(
        #[case] polls: Vec<HardwareMessage>,
        #[case] effects: Vec<HardwareEffect>,
    ) {
        let mut hardware = HardwareState::default();
        let observed: Vec<HardwareEffect> = polls
            .into_iter()
            .map(|message| hardware.apply(message))
            .collect();
        assert_eq!(observed, effects);
    }
}
