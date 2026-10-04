#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use kernel::{cmd::Playback, domain::track::Track};
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_media_player::{MPMediaItemArtwork, MPNowPlayingPlaybackState};

use crate::{clock::NowPlayingClock, ffi};

const PLACEHOLDER_TITLE: &str = "sifr";
const PLAYING_RATE: f64 = 1.0;
const PAUSED_RATE: f64 = 0.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct NowPlaying<'a> {
    pub(crate) track: Option<&'a Track>,
    pub(crate) clock: NowPlayingClock,
    pub(crate) artwork: Option<&'a MPMediaItemArtwork>,
}

fn rate(playback: Playback) -> f64 {
    match playback {
        Playback::Playing => PLAYING_RATE,
        Playback::Paused => PAUSED_RATE,
    }
}

pub(crate) fn now_playing_info(
    shown: NowPlaying<'_>,
    now: Instant,
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let (title, artist, album, duration) = shown.track.map_or_else(
        || (PLACEHOLDER_TITLE.to_owned(), None, None, Duration::ZERO),
        |track| {
            (
                track.song_title(),
                track.tags().artist.as_deref(),
                track.tags().album.as_deref(),
                track.duration().unwrap_or(Duration::ZERO),
            )
        },
    );
    let title = NSString::from_str(&title);
    let artist = artist.map(NSString::from_str);
    let album = album.map(NSString::from_str);
    let duration = NSNumber::new_f64(duration.as_secs_f64());
    let elapsed = NSNumber::new_f64(shown.clock.elapsed(now).as_secs_f64());
    let rate = NSNumber::new_f64(rate(shown.clock.playback()));
    let keys = [
        ffi::title_key(),
        ffi::duration_key(),
        ffi::elapsed_key(),
        ffi::rate_key(),
        ffi::artist_key(),
        ffi::album_key(),
        ffi::artwork_key(),
    ];
    let values: [Option<&AnyObject>; 7] = [
        Some(&title),
        Some(&duration),
        Some(&elapsed),
        Some(&rate),
        artist.as_deref().map(AsRef::as_ref),
        album.as_deref().map(AsRef::as_ref),
        shown.artwork.map(AsRef::as_ref),
    ];
    let (names, objects): (Vec<&NSString>, Vec<&AnyObject>) = keys
        .into_iter()
        .zip(values)
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .unzip();
    NSDictionary::from_slices(&names, &objects)
}

pub(crate) fn publish(shown: NowPlaying<'_>, now: Instant) {
    let info = now_playing_info(shown, now);
    let state = match shown.clock.playback() {
        Playback::Playing => MPNowPlayingPlaybackState::Playing,
        Playback::Paused => MPNowPlayingPlaybackState::Paused,
    };
    let center = ffi::now_playing_info_center();
    ffi::publish_now_playing_info(&center, &info);
    ffi::publish_playback_state(&center, state);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use kernel::{
        cmd::Playback,
        domain::track::{AudioFormat, Tags, Track},
    };
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDictionary, NSNumber, NSString};

    use crate::{
        clock::NowPlayingClock,
        ffi,
        now_playing::{NowPlaying, PLACEHOLDER_TITLE, PLAYING_RATE, now_playing_info},
    };

    fn text(
        dictionary: &NSDictionary<NSString, AnyObject>,
        key: &NSString,
    ) -> Option<String> {
        let found = dictionary.objectForKey(key)?;
        Some(found.downcast_ref::<NSString>()?.to_string())
    }

    fn number(
        dictionary: &NSDictionary<NSString, AnyObject>,
        key: &NSString,
    ) -> Option<f64> {
        let found = dictionary.objectForKey(key)?;
        Some(found.downcast_ref::<NSNumber>()?.as_f64())
    }

    #[test]
    fn a_cleared_now_playing_still_carries_a_title() {
        let start = Instant::now();
        let shown = NowPlaying {
            track: None,
            clock: NowPlayingClock::default(),
            artwork: None,
        };
        let info = now_playing_info(shown, start);
        let title = ffi::title_key();
        assert_eq!(text(&info, title).as_deref(), Some(PLACEHOLDER_TITLE));
    }

    #[test]
    fn a_track_carries_the_tags_it_has_and_where_the_clock_is() {
        let track = Track::builder()
            .path("/tmp/tuonela.flac")
            .duration(Duration::from_secs(42))
            .tags(Tags {
                title: Some("Tuonela".to_owned()),
                artist: Some("Amorphis".to_owned()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build();
        let start = Instant::now();
        let shown = NowPlaying {
            track: Some(&track),
            clock: NowPlayingClock::default()
                .seek(Duration::from_secs(7), start)
                .change_playback(Playback::Playing, start),
            artwork: None,
        };
        let info = now_playing_info(shown, start + Duration::from_secs(3));
        let (title, artist, album, elapsed, rate) = (
            ffi::title_key(),
            ffi::artist_key(),
            ffi::album_key(),
            ffi::elapsed_key(),
            ffi::rate_key(),
        );
        assert_eq!(text(&info, title).as_deref(), Some("Tuonela"));
        assert_eq!(text(&info, artist).as_deref(), Some("Amorphis"));
        assert!(info.objectForKey(album).is_none());
        assert_eq!(number(&info, elapsed), Some(10.0));
        assert_eq!(number(&info, rate), Some(PLAYING_RATE));
    }
}
