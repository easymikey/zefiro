#![forbid(unsafe_code)]

mod cache;
pub mod cover;
pub mod dirs;
pub mod driver;
pub mod error;
mod execute;
mod favorites;
pub mod files;
mod history;
pub mod job;
pub mod message;
pub mod playlists;
pub mod scan;
pub mod tags;
mod trash;
mod watch;

#[cfg(test)]
mod test_support {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::track::{AudioFormat, Tags, Track, TrackParts};

    const FIXTURE_LENGTH: Duration = Duration::from_secs(180);

    #[must_use]
    pub(crate) fn track_lasting(path: &str, duration: Duration, tags: Tags) -> Track {
        Track::new(TrackParts {
            path: path.into(),
            duration,
            tags,
            audio_format: AudioFormat::default(),
        })
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
    pub(crate) fn minimal_flac_with_cover(picture_data: &[u8]) -> Vec<u8> {
        let length =
            |bytes: &[u8]| u32::try_from(bytes.len()).unwrap_or(0).to_be_bytes();
        let mime: &[u8] = b"image/jpeg";
        let picture_payload = [
            &3u32.to_be_bytes()[..],
            &length(mime),
            mime,
            &length(b""),
            &1u32.to_be_bytes(),
            &1u32.to_be_bytes(),
            &24u32.to_be_bytes(),
            &0u32.to_be_bytes(),
            &length(picture_data),
            picture_data,
        ]
        .concat();
        let streaminfo_bits: u64 = (44100u64 << 44) | (1u64 << 41) | (15u64 << 36);
        let streaminfo = [
            &4096u16.to_be_bytes()[..],
            &4096u16.to_be_bytes(),
            &[0, 0, 0],
            &[0, 0, 0],
            &streaminfo_bits.to_be_bytes(),
            &[0u8; 16],
        ]
        .concat();
        let payload_len = picture_payload.len().to_be_bytes();
        [
            &b"fLaC"[..],
            &[0x00],
            &[0x00, 0x00, 0x22],
            &streaminfo,
            &[0x86],
            &payload_len[payload_len.len() - 3..],
            &picture_payload,
        ]
        .concat()
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
}
