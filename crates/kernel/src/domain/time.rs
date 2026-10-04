use std::time::Duration;

pub(crate) const SECONDS_PER_MINUTE: u64 = 60;

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
    pub fn new(since_epoch: Duration) -> Self {
        Self(since_epoch)
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

    use crate::domain::time::{Moment, TimecodeError, parse_timecode};

    #[test]
    fn parse_timecode_accepts_every_documented_shape() {
        assert_eq!(parse_timecode("1:05"), Ok(Duration::from_secs(65)));
        assert_eq!(parse_timecode("01:05"), Ok(Duration::from_secs(65)));
        assert_eq!(parse_timecode("1:02:03"), Ok(Duration::from_secs(3723)));
        assert_eq!(parse_timecode("90"), Ok(Duration::from_secs(90)));
    }

    #[test]
    fn parse_timecode_rejects_an_out_of_range_seconds_field() {
        assert_eq!(
            parse_timecode("1:99"),
            Err(TimecodeError::OutOfRange { value: 99, max: 59 })
        );
    }

    #[test]
    fn parse_timecode_rejects_non_numeric_input() {
        assert_eq!(parse_timecode("abc"), Err(TimecodeError::Malformed));
    }

    #[test]
    fn parse_timecode_rejects_empty_input() {
        assert_eq!(parse_timecode(""), Err(TimecodeError::Empty));
    }

    #[test]
    fn since_epoch_returns_the_stored_duration() {
        let moment = Moment::new(Duration::from_secs(3));
        assert_eq!(moment.since_epoch(), Duration::from_secs(3));
    }

    #[test]
    fn default_is_the_epoch_itself() {
        assert_eq!(Moment::default(), Moment::new(Duration::ZERO));
    }

    #[test]
    fn elapsed_since_is_the_gap_between_two_moments() {
        let earlier = Moment::new(Duration::from_secs(3));
        let later = Moment::new(Duration::from_secs(5));
        assert_eq!(later.elapsed_since(earlier), Duration::from_secs(2));
    }

    #[test]
    fn elapsed_since_saturates_when_the_other_moment_is_later() {
        let earlier = Moment::new(Duration::from_secs(3));
        let later = Moment::new(Duration::from_secs(5));
        assert_eq!(earlier.elapsed_since(later), Duration::ZERO);
    }
}
