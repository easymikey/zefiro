#![forbid(unsafe_code)]

use kernel::update::{Machine, Rejected};
use objc2_core_audio::AudioObjectID;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum DefaultOutput {
    #[default]
    Unread,
    Read(AudioObjectID),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputMessage(pub(crate) AudioObjectID);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum OutputEffect {
    #[default]
    Nothing,
    Changed,
}

impl Machine for DefaultOutput {
    type Message = OutputMessage;
    type Error = std::convert::Infallible;
    type Effect = OutputEffect;

    fn transition(
        self,
        message: OutputMessage,
    ) -> Result<(Self, OutputEffect), Rejected<Self>> {
        let OutputMessage(current) = message;
        let effect = match self {
            DefaultOutput::Read(previous) if previous != current => {
                OutputEffect::Changed
            }
            DefaultOutput::Unread | DefaultOutput::Read(_) => OutputEffect::Nothing,
        };
        Ok((DefaultOutput::Read(current), effect))
    }
}

#[cfg(test)]
mod tests {
    use kernel::update::Machine;
    use objc2_core_audio::AudioObjectID;
    use rstest::rstest;

    use crate::output::{DefaultOutput, OutputEffect, OutputMessage};

    #[rstest]
    #[case::the_first_read_is_the_baseline(&[1], &[OutputEffect::Nothing])]
    #[case::the_same_device(
        &[1, 1],
        &[OutputEffect::Nothing, OutputEffect::Nothing]
    )]
    #[case::headphones_plugged_in(
        &[1, 2],
        &[OutputEffect::Nothing, OutputEffect::Changed]
    )]
    #[case::back_to_the_first_device(
        &[1, 2, 1],
        &[OutputEffect::Nothing, OutputEffect::Changed, OutputEffect::Changed]
    )]
    fn only_a_different_default_output_is_a_change(
        #[case] polls: &[AudioObjectID],
        #[case] effects: &[OutputEffect],
    ) {
        let mut output = DefaultOutput::default();
        let observed: Vec<OutputEffect> = polls
            .iter()
            .map(|device| {
                let Ok(effect) = output.update(OutputMessage(*device));
                effect
            })
            .collect();
        assert_eq!(observed, effects);
    }
}
