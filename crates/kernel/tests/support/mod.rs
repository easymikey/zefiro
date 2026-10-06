pub(crate) mod keymap;
pub(crate) mod router;
pub(crate) mod strategies;
pub(crate) mod table;
pub(crate) mod update;

use std::{path::Path, sync::Arc, time::Duration};

use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        cursor::Cursor,
        device::DeviceName,
        model::Model,
        player::Player,
        playhead::Playhead,
        playlist::Playlist,
        revision::Revision,
        speed::Speed,
        time::Moment,
        toast::TOAST_LIFETIME,
        track::{AudioFormat, Tags, Track, TrackParts},
    },
    message::Timer,
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
    Arc::new(Track::new(TrackParts {
        path: path.into(),
        duration,
        tags: Tags::default(),
        audio_format: AudioFormat::default(),
    }))
}

pub(crate) fn dated_track(number: usize) -> Arc<Track> {
    track_with_duration(&format!("/tmp/track{number}.flac"), FIXTURE_LENGTH)
}

pub(crate) fn titled_track(path: &str, title: &str, artist: &str) -> Arc<Track> {
    let artist = (!artist.is_empty()).then(|| artist.to_string());
    Arc::new(Track::new(TrackParts {
        path: path.into(),
        duration: FIXTURE_LENGTH,
        tags: Tags {
            title: Some(title.to_string()),
            artist,
            ..Tags::default()
        },
        audio_format: AudioFormat::default(),
    }))
}

pub(crate) fn track_with_tags(path: &str, tags: Tags) -> Arc<Track> {
    Arc::new(Track::new(TrackParts {
        path: path.into(),
        duration: FIXTURE_LENGTH,
        tags,
        audio_format: AudioFormat::default(),
    }))
}

pub(crate) fn listed_model(playlist_tracks: Vec<Arc<Track>>) -> Model {
    let mut model = Model {
        playlist: Playlist::from_tracks(playlist_tracks),
        ..Default::default()
    };
    let browse = &mut model.workspace.browse;
    browse.cursor = browse.cursor.resize(model.playlist.tracks.len());
    model
}

pub(crate) fn model_with_titled_tracks(tracks: &[(&str, &str, &str)]) -> Model {
    listed_model(
        tracks
            .iter()
            .map(|(path, title, artist)| titled_track(path, title, artist))
            .collect(),
    )
}

pub(crate) fn model_with_tracks(count: usize) -> Model {
    listed_model((0..count).map(bare_track).collect())
}

pub(crate) fn model_with_dated_tracks(count: usize) -> Model {
    listed_model((0..count).map(dated_track).collect())
}

pub(crate) fn model_playing_at(
    count: usize,
    playing_index: usize,
    position: Duration,
) -> Model {
    let mut model = model_with_dated_tracks(count);
    model.playlist.cursor = Cursor::at(count, playing_index);
    model.player = Player::Playing {
        track: dated_track(playing_index),
        playhead: Playhead::anchored(position, Moment::default(), Speed::default()),
        preloaded: None,
    };
    model
}

pub(crate) fn playing_model(count: usize) -> Model {
    let mut model = model_with_dated_tracks(count);
    model.player = Player::Playing {
        track: dated_track(0),
        playhead: Playhead::anchored(
            Duration::ZERO,
            Moment::default(),
            Speed::default(),
        ),
        preloaded: None,
    };
    model
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
