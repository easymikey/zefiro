use std::time::Duration;

pub const SECONDS_PER_MINUTE: u64 = 60;

const MINUTES_PER_HOUR: u64 = 60;

const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * MINUTES_PER_HOUR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TimecodeError {
    #[error("enter a time")]
    Empty,
    #[error("not a valid time (use m:ss, mm:ss, h:mm:ss, or plain seconds)")]
    Malformed,
    #[error("minutes/seconds field {value} is above {max}")]
    OutOfRange { value: u64, max: u64 },
}

fn bounded(field: u64, max: u64) -> Result<u64, TimecodeError> {
    if field > max {
        Err(TimecodeError::OutOfRange { value: field, max })
    } else {
        Ok(field)
    }
}

pub(crate) fn parse_timecode(input: &str) -> Result<Duration, TimecodeError> {
    if input.is_empty() {
        return Err(TimecodeError::Empty);
    }
    let fields: Vec<&str> = input.split(':').collect();
    let total_seconds = match fields.as_slice() {
        [seconds_only] => parse_field(seconds_only)?,
        [minutes, seconds] => {
            let minutes = parse_field(minutes)?;
            let seconds = bounded(parse_field(seconds)?, SECONDS_PER_MINUTE - 1)?;
            minutes * SECONDS_PER_MINUTE + seconds
        }
        [hours, minutes, seconds] => {
            let hours = parse_field(hours)?;
            let minutes = bounded(parse_field(minutes)?, MINUTES_PER_HOUR - 1)?;
            let seconds = bounded(parse_field(seconds)?, SECONDS_PER_MINUTE - 1)?;
            hours * SECONDS_PER_HOUR + minutes * SECONDS_PER_MINUTE + seconds
        }
        _ => return Err(TimecodeError::Malformed),
    };
    Ok(Duration::from_secs(total_seconds))
}

fn parse_field(field: &str) -> Result<u64, TimecodeError> {
    field.parse::<u64>().map_err(|_| TimecodeError::Malformed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Moment(Duration);

impl Moment {
    #[must_use]
    pub fn new(elapsed: Duration) -> Self {
        Self(elapsed)
    }

    #[must_use]
    pub fn since_epoch(self) -> Duration {
        self.0
    }

    #[must_use]
    pub fn elapsed_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::time::{Moment, TimecodeError, parse_timecode};

    #[test]
    fn parse_timecode_accepts_every_documented_shape() {
        assert_eq!(parse_timecode("1:05"), Ok(Duration::from_secs(65)));
        assert_eq!(parse_timecode("01:05"), Ok(Duration::from_secs(65)));
        assert_eq!(parse_timecode("1:02:03"), Ok(Duration::from_secs(3723)));
        assert_eq!(parse_timecode("90"), Ok(Duration::from_secs(90)));
    }

    #[rstest]
    #[case::in_minutes_and_seconds("1:99", 99)]
    #[case::in_hours_minutes_and_seconds("1:00:60", 60)]
    fn parse_timecode_rejects_an_out_of_range_seconds_field(
        #[case] input: &str,
        #[case] seconds: u64,
    ) {
        assert_eq!(
            parse_timecode(input),
            Err(TimecodeError::OutOfRange {
                value: seconds,
                max: 59
            })
        );
    }

    #[test]
    fn since_epoch_returns_the_stored_duration() {
        let elapsed = Duration::from_secs(90);
        assert_eq!(Moment::new(elapsed).since_epoch(), elapsed);
    }

    #[test]
    fn parse_timecode_rejects_empty_input() {
        assert_eq!(parse_timecode(""), Err(TimecodeError::Empty));
    }

    #[rstest]
    #[case(5, 3, 2)]
    #[case(3, 5, 0)]
    fn elapsed_since_is_the_gap_between_two_moments(
        #[case] at: u64,
        #[case] other: u64,
        #[case] gap: u64,
    ) {
        assert_eq!(
            Moment::new(Duration::from_secs(at))
                .elapsed_since(Moment::new(Duration::from_secs(other))),
            Duration::from_secs(gap)
        );
    }
}
