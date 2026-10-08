use std::time::Duration;

use crate::domain::{speed::Speed, time::Moment};

#[must_use]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    pub offset: Duration,
    pub started_at: Moment,
    pub speed: Speed,
}

impl Playhead {
    pub fn anchored(offset: Duration, started_at: Moment, speed: Speed) -> Self {
        Self {
            offset,
            started_at,
            speed,
        }
    }

    #[must_use]
    pub fn position_at(self, now: Moment) -> Duration {
        let elapsed = now.elapsed_since(self.started_at);
        self.offset + elapsed.mul_f32(self.speed.get())
    }
}
