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
    #[error("minutes/seconds field must be less than 60")]
    SecondsOutOfRange,
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
            let seconds = parse_field(seconds)?;
            if seconds >= SECONDS_PER_MINUTE {
                return Err(TimecodeError::SecondsOutOfRange);
            }
            minutes * SECONDS_PER_MINUTE + seconds
        }
        [hours, minutes, seconds] => {
            let hours = parse_field(hours)?;
            let minutes = parse_field(minutes)?;
            let seconds = parse_field(seconds)?;
            if minutes >= MINUTES_PER_HOUR || seconds >= SECONDS_PER_MINUTE {
                return Err(TimecodeError::SecondsOutOfRange);
            }
            hours * SECONDS_PER_HOUR + minutes * SECONDS_PER_MINUTE + seconds
        }
        _ => return Err(TimecodeError::Malformed),
    };
    Ok(Duration::from_secs(total_seconds))
}

fn parse_field(field: &str) -> Result<u64, TimecodeError> {
    field.parse::<u64>().map_err(|_| TimecodeError::Malformed)
}

#[must_use]
pub fn format_time(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let (hours, minutes, seconds) = (
        total_secs / SECONDS_PER_HOUR,
        (total_secs % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE,
        total_secs % SECONDS_PER_MINUTE,
    );
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::rstest;

    use crate::domain::time::{TimecodeError, format_time, parse_timecode};

    #[rstest]
    #[case(0, "00:00")]
    #[case(65, "01:05")]
    #[case(3599, "59:59")]
    #[case(3600, "1:00:00")]
    #[case(3661, "1:01:01")]
    #[case(7384, "2:03:04")]
    fn format_time_grows_an_hours_field_only_when_there_is_one(
        #[case] seconds: u64,
        #[case] expected: &str,
    ) {
        assert_eq!(format_time(Duration::from_secs(seconds)), expected);
    }

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
            Err(TimecodeError::SecondsOutOfRange)
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
}
