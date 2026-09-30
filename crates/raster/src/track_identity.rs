use std::hash::{Hash, Hasher};

use kernel::domain::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackIdentity(u64);

impl TrackIdentity {
    #[must_use]
    pub fn of(track: &Track) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track.path().hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{AudioFormat, Tags, Track};

    use crate::track_identity::TrackIdentity;

    fn track(path: &str) -> Track {
        Track::builder()
            .path(path)
            .duration(std::time::Duration::from_secs(1))
            .tags(Tags::default())
            .audio_format(AudioFormat::default())
            .build()
    }

    #[test]
    fn two_tracks_at_the_same_point_carry_different_identities() {
        let one = TrackIdentity::of(&track("one.mp3"));
        let other = TrackIdentity::of(&track("other.mp3"));
        assert_ne!(
            one, other,
            "the path decides which track an image belongs to"
        );
    }
}
