#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use kernel::{NowPlaying, Playback};
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_media_player::{MPMediaItemArtwork, MPNowPlayingPlaybackState};

use crate::{clock::PanelClock, ffi};

const PLACEHOLDER_TITLE: &str = "sifr";
const PLAYING_RATE: f64 = 1.0;
const PAUSED_RATE: f64 = 0.0;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Panel<'a> {
    pub(crate) showing: &'a NowPlaying,
    pub(crate) clock: PanelClock,
    pub(crate) artwork: Option<&'a MPMediaItemArtwork>,
}

struct Tags<'a> {
    title: &'a str,
    artist: Option<&'a str>,
    album: Option<&'a str>,
    duration: Duration,
}

impl<'a> From<&'a NowPlaying> for Tags<'a> {
    fn from(now_playing: &'a NowPlaying) -> Self {
        match now_playing {
            NowPlaying::Cleared => Self {
                title: PLACEHOLDER_TITLE,
                artist: None,
                album: None,
                duration: Duration::ZERO,
            },
            NowPlaying::Track {
                title,
                artist,
                album,
                duration,
                ..
            } => Self {
                title: title.as_str(),
                artist: artist.as_deref(),
                album: album.as_deref(),
                duration: *duration,
            },
        }
    }
}

fn rate(playback: Playback) -> f64 {
    match playback {
        Playback::Playing => PLAYING_RATE,
        Playback::Paused => PAUSED_RATE,
    }
}

pub(crate) fn now_playing_info(
    panel: Panel<'_>,
    now: Instant,
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let tags = Tags::from(panel.showing);
    let title = NSString::from_str(tags.title);
    let artist = tags.artist.map(NSString::from_str);
    let album = tags.album.map(NSString::from_str);
    let duration = NSNumber::new_f64(tags.duration.as_secs_f64());
    let elapsed = NSNumber::new_f64(panel.clock.elapsed(now).as_secs_f64());
    let rate = NSNumber::new_f64(rate(panel.clock.playback()));
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
        panel.artwork.map(AsRef::as_ref),
    ];
    let (names, objects): (Vec<&NSString>, Vec<&AnyObject>) = keys
        .into_iter()
        .zip(values)
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .unzip();
    NSDictionary::from_slices(&names, &objects)
}

pub(crate) fn publish(panel: Panel<'_>, now: Instant) {
    let info = now_playing_info(panel, now);
    let state = match panel.clock.playback() {
        Playback::Playing => MPNowPlayingPlaybackState::Playing,
        Playback::Paused => MPNowPlayingPlaybackState::Paused,
    };
    let center = ffi::now_playing_info_center();
    ffi::publish_now_playing_info(&center, &info);
    ffi::publish_playback_state(&center, state);
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    use kernel::{NowPlaying, Playback};
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSDictionary, NSNumber, NSString};

    use crate::{
        clock::PanelClock,
        ffi,
        now_playing::{PLACEHOLDER_TITLE, PLAYING_RATE, Panel, now_playing_info},
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
    fn a_cleared_panel_still_carries_a_title() {
        let start = Instant::now();
        let panel = Panel {
            showing: &NowPlaying::Cleared,
            clock: PanelClock::new(start),
            artwork: None,
        };
        let info = now_playing_info(panel, start);
        let title = ffi::title_key();
        assert_eq!(text(&info, title).as_deref(), Some(PLACEHOLDER_TITLE));
    }

    #[test]
    fn a_track_carries_the_tags_it_has_and_where_the_clock_is() {
        let track = NowPlaying::Track {
            title: "Tuonela".to_owned(),
            artist: Some("Amorphis".to_owned()),
            album: None,
            duration: Duration::from_secs(42),
            path: PathBuf::from("/tmp/tuonela.flac"),
        };
        let start = Instant::now();
        let panel = Panel {
            showing: &track,
            clock: PanelClock::new(start)
                .seek(Duration::from_secs(7), start)
                .with_playback(Playback::Playing, start),
            artwork: None,
        };
        let info = now_playing_info(panel, start + Duration::from_secs(3));
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
