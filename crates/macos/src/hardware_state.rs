#![forbid(unsafe_code)]

use kernel::{
    MacosEvent,
    Percent,
    update::{Machine, Rejected},
};
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

impl Machine for VolumeEcho {
    type Message = VolumeMessage;
    type Error = std::convert::Infallible;
    type Effect = VolumeEffect;

    fn transition(
        self,
        message: VolumeMessage,
    ) -> Result<(Self, VolumeEffect), Rejected<Self>> {
        Ok(match message {
            VolumeMessage::Written(volume) => (
                Self {
                    pending: Some(volume),
                    last_reported: self.last_reported,
                },
                VolumeEffect::Nothing,
            ),
            VolumeMessage::Polled(volume) if self.pending == Some(volume) => (
                Self {
                    pending: None,
                    last_reported: Some(volume),
                },
                VolumeEffect::Nothing,
            ),
            VolumeMessage::Polled(volume) if self.last_reported == Some(volume) => {
                (self, VolumeEffect::Nothing)
            }
            VolumeMessage::Polled(volume) => (
                Self {
                    pending: self.pending,
                    last_reported: Some(volume),
                },
                VolumeEffect::Changed(volume),
            ),
        })
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

impl Machine for HardwareState {
    type Message = HardwareMessage;
    type Error = std::convert::Infallible;
    type Effect = HardwareEffect;

    fn transition(
        self,
        message: HardwareMessage,
    ) -> Result<(Self, HardwareEffect), Rejected<Self>> {
        let (echo, volume) = message.volume.map_or((self.echo, None), |polled| {
            let Ok((echo, effect)) =
                self.echo.transition(VolumeMessage::Polled(polled));
            let event = match effect {
                VolumeEffect::Changed(reported) => Some(MacosEvent::Volume(reported)),
                VolumeEffect::Nothing => None,
            };
            (echo, event)
        });
        let route = self
            .device
            .is_some_and(|previous| previous != message.current_device)
            .then_some(MacosEvent::OutputRouteChanged);
        let rebind = (message.tracked_device != message.current_device)
            .then_some(message.current_device);
        Ok((
            Self {
                echo,
                device: Some(message.current_device),
            },
            HardwareEffect {
                rebind,
                events: [volume, route].into_iter().flatten().collect(),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, MacosEvent, Percent, update::Machine};
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

    fn percent(value: u8) -> Percent {
        Percent::clamped(value)
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
            echo.update(VolumeMessage::Polled(percent(30))),
            Ok(VolumeEffect::Changed(percent(30)))
        );
        assert_eq!(
            echo.update(VolumeMessage::Written(percent(40))),
            Ok(VolumeEffect::Nothing)
        );
        let observed: Vec<VolumeEffect> = polls
            .iter()
            .map(|value| {
                let Ok(effect) = echo.update(VolumeMessage::Polled(percent(*value)));
                effect
            })
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
            .map(|message| {
                let Ok(effect) = hardware.update(message);
                effect
            })
            .collect();
        assert_eq!(observed, effects);
    }
}
