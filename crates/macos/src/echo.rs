#![forbid(unsafe_code)]

use kernel::{
    Percent,
    update::{Machine, Never, Rejected},
};

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum VolumeEffect {
    #[default]
    Nothing,
    Report(Percent),
}

impl Machine for VolumeEcho {
    type Message = VolumeMessage;
    type Rejection = Never;
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
                VolumeEffect::Report(volume),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, Percent, update::Machine};
    use rstest::rstest;

    use crate::echo::{VolumeEcho, VolumeEffect, VolumeMessage};

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
        &[VolumeEffect::Nothing, VolumeEffect::Report(Percent::clamped(55))]
    )]
    fn volume_echo_suppresses_our_own_write(
        #[case] polls: &[u8],
        #[case] reported: &[VolumeEffect],
    ) {
        let mut echo = VolumeEcho::default();
        assert_eq!(
            echo.update(VolumeMessage::Polled(percent(30))),
            Ok(VolumeEffect::Report(percent(30)))
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
}
