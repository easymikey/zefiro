pub(crate) mod keymap;
pub(crate) mod router;
pub(crate) mod step;
pub(crate) mod strategies;
pub(crate) mod table;

use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    Cmd,
    Effect,
    Model,
    Moment,
    Player,
    Playhead,
    Preload,
    Speed,
    TOAST_LIFETIME,
    Timer,
    Track,
    domain::{AudioFormat, Cursor, DeviceName, Revision, Tags},
    playlist::Playlist,
};

const FIXTURE_LENGTH: Duration = Duration::from_secs(100);

pub(crate) fn device(name: &str) -> DeviceName {
    DeviceName::new(name.to_string()).unwrap()
}

pub(crate) fn bare_track(number: usize) -> Arc<Track> {
    track_at(&format!("/tmp/track{number}.flac"))
}

pub(crate) fn track_at(path: &str) -> Arc<Track> {
    Arc::new(Track::listed(Path::new(path)))
}

pub(crate) fn track_with_duration(path: &str, duration: Duration) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(path)
            .duration(duration)
            .tags(Tags::default())
            .audio_format(AudioFormat::default())
            .build(),
    )
}

pub(crate) fn dated_track(number: usize) -> Arc<Track> {
    track_with_duration(&format!("/tmp/track{number}.flac"), FIXTURE_LENGTH)
}

pub(crate) fn titled_track(path: &str, title: &str, artist: &str) -> Arc<Track> {
    let artist = (!artist.is_empty()).then(|| artist.to_string());
    Arc::new(
        Track::builder()
            .path(path)
            .duration(FIXTURE_LENGTH)
            .tags(Tags {
                title: Some(title.to_string()),
                artist,
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    )
}

pub(crate) fn track_with_tags(path: &str, tags: Tags) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(path)
            .duration(FIXTURE_LENGTH)
            .tags(tags)
            .audio_format(AudioFormat::default())
            .build(),
    )
}

pub(crate) fn model_with_titled_tracks(tracks: &[(&str, &str, &str)]) -> Model {
    Model {
        playlist: Playlist::from_tracks(
            tracks
                .iter()
                .map(|(path, title, artist)| titled_track(path, title, artist))
                .collect(),
        ),
        ..Default::default()
    }
}

pub(crate) fn model_with_tracks(count: usize) -> Model {
    Model {
        playlist: Playlist::from_tracks((0..count).map(bare_track).collect()),
        ..Default::default()
    }
}

pub(crate) fn model_with_dated_tracks(count: usize) -> Model {
    Model {
        playlist: Playlist::from_tracks((0..count).map(dated_track).collect()),
        ..Default::default()
    }
}

pub(crate) fn model_playing_at(count: usize, k: usize, at: Duration) -> Model {
    let mut m = model_with_dated_tracks(count);
    m.playlist.cursor = Cursor::with_len(count).at(k);
    m.player = Player::Playing {
        track: dated_track(k),
        head: Playhead::anchored(at, Moment::default(), Speed::default()),
        preload: Preload::None,
    };
    m
}

pub(crate) fn playing_model(count: usize) -> Model {
    let mut m = model_with_dated_tracks(count);
    m.player = Player::Playing {
        track: dated_track(0),
        head: Playhead::anchored(Duration::ZERO, Moment::default(), Speed::default()),
        preload: Preload::None,
    };
    m
}

pub(crate) fn effects(cmd: Cmd) -> Vec<Effect> {
    cmd.into_parts().0
}

pub(crate) fn first_toast_expiry() -> Effect {
    Effect::After {
        delay: TOAST_LIFETIME,
        timer: Timer::Toast(Revision::default().next()),
    }
}
