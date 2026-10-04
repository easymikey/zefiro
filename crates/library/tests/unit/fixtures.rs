use std::{sync::Arc, time::Duration};

use kernel::{AudioFormat, Tags, Track};

const FIXTURE_LENGTH: Duration = Duration::from_secs(180);

#[must_use]
pub(crate) fn track_lasting(path: &str, length: Duration, tags: Tags) -> Track {
    Track::builder()
        .path(path)
        .duration(length)
        .tags(tags)
        .audio_format(AudioFormat::default())
        .build()
}

#[must_use]
pub(crate) fn track(path: &str, tags: Tags) -> Track {
    track_lasting(path, FIXTURE_LENGTH, tags)
}

#[must_use]
pub(crate) fn titled(path: &str, title: &str) -> Arc<Track> {
    Arc::new(track(
        path,
        Tags {
            title: Some(title.to_string()),
            ..Tags::default()
        },
    ))
}

#[must_use]
pub(crate) fn temp_dir_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            r"(/private)?/var/folders/[^/]+/[^/]+/T/\.tmp[A-Za-z0-9]+",
            "[tmp]",
        ),
        (r"/tmp/\.tmp[A-Za-z0-9]+", "[tmp]"),
    ]
}
