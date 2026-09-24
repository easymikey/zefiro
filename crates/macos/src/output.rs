#![forbid(unsafe_code)]

use cpal::traits::{DeviceTrait, HostTrait};
use kernel::update::{Machine, Never, Rejected};

pub(crate) fn default_output_name() -> Option<String> {
    cpal::default_host().default_output_device()?.name().ok()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum DefaultOutput {
    #[default]
    Unread,
    Read(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputPolled(pub(crate) Option<String>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum OutputEffect {
    #[default]
    Nothing,
    Changed,
}

impl Machine for DefaultOutput {
    type Message = OutputPolled;
    type Rejection = Never;
    type Effect = OutputEffect;

    fn transition(
        self,
        message: OutputPolled,
    ) -> Result<(Self, OutputEffect), Rejected<Self>> {
        let OutputPolled(current) = message;
        let effect = match &self {
            DefaultOutput::Read(previous) if *previous != current => {
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
    use rstest::rstest;

    use crate::output::{DefaultOutput, OutputEffect, OutputPolled};

    #[rstest]
    #[case::the_first_read_is_the_baseline(&[Some("Speakers")], &[OutputEffect::Nothing])]
    #[case::the_same_device(
        &[Some("Speakers"), Some("Speakers")],
        &[OutputEffect::Nothing, OutputEffect::Nothing]
    )]
    #[case::headphones_plugged_in(
        &[Some("Speakers"), Some("Headphones")],
        &[OutputEffect::Nothing, OutputEffect::Changed]
    )]
    #[case::the_device_disappears(
        &[Some("DAC"), None, None],
        &[OutputEffect::Nothing, OutputEffect::Changed, OutputEffect::Nothing]
    )]
    fn only_a_different_default_output_is_a_change(
        #[case] polls: &[Option<&str>],
        #[case] effects: &[OutputEffect],
    ) {
        let mut output = DefaultOutput::default();
        let observed: Vec<OutputEffect> = polls
            .iter()
            .map(|name| {
                let Ok(effect) = output.update(OutputPolled(name.map(str::to_owned)));
                effect
            })
            .collect();
        assert_eq!(observed, effects);
    }
}
