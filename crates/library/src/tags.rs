use std::{borrow::Cow, path::Path};

use kernel::{LibrarySubject, Track};
use lofty::{
    config::ParseOptions,
    prelude::{AudioFile, TaggedFileExt},
    probe::Probe,
    tag::{Accessor, ItemKey, Tag},
};

use crate::error::LibraryError;

fn owned_tag(tag: Option<Cow<'_, str>>) -> Option<String> {
    tag.map(Cow::into_owned)
}

pub(crate) fn read_track(path: &Path) -> Result<Track, LibraryError> {
    let file = std::fs::File::open(path).map_err(|source| LibraryError::Read {
        subject: LibrarySubject::Scan,
        path: path.to_path_buf(),
        source,
    })?;
    let options = ParseOptions::new().read_cover_art(false);
    let Ok(tagged) = Probe::new(std::io::BufReader::new(file))
        .options(options)
        .guess_file_type()
        .map_err(|_| ())
        .and_then(|probe| probe.read().map_err(|_| ()))
    else {
        return Ok(Track::listed(path));
    };
    let properties = tagged.properties();
    let duration = properties.duration();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let audio_format = kernel::AudioFormat {
        format: Some(format!("{:?}", tagged.file_type())),
        sample_rate_hz: properties.sample_rate(),
        bitrate_kbps: properties.audio_bitrate(),
        bits_per_sample: properties.bit_depth(),
        channels: properties.channels(),
        replay_gain: tag.and_then(replay_gain_of),
    };
    let tags = tag.map_or_else(kernel::Tags::default, tags_from);
    Ok(Track::builder()
        .path(path)
        .duration(duration)
        .tags(tags)
        .audio_format(audio_format)
        .build())
}

fn tags_from(tag: &Tag) -> kernel::Tags {
    kernel::Tags {
        title: owned_tag(tag.title()),
        artist: owned_tag(tag.artist()),
        album: owned_tag(tag.album()),
        genre: owned_tag(tag.genre()),
        comment: owned_tag(tag.comment()),
        lyrics: tag.get_string(ItemKey::Lyrics).map(str::to_owned),
        composer: tag.get_string(ItemKey::Composer).map(str::to_owned),
        album_artist: tag.get_string(ItemKey::AlbumArtist).map(str::to_owned),
        date: tag
            .get_string(ItemKey::Year)
            .or_else(|| tag.get_string(ItemKey::OriginalReleaseDate))
            .or_else(|| tag.get_string(ItemKey::ReleaseDate))
            .map(str::to_owned),
        track: tag.track(),
        track_total: tag.track_total(),
        disc: tag.disk(),
    }
}

fn replay_gain_of(tag: &Tag) -> Option<f32> {
    tag.get_string(ItemKey::ReplayGainTrackGain)
        .and_then(parse_replay_gain)
}

fn parse_replay_gain(raw: &str) -> Option<f32> {
    raw.trim()
        .trim_end_matches(|ch: char| ch.is_ascii_alphabetic())
        .trim()
        .parse()
        .ok()
}

#[must_use]
pub fn embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let picture = tag.pictures().first()?;
    Some(picture.data().to_vec())
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use crate::tags::{embedded_cover, parse_replay_gain, read_track};

    #[fixture]
    fn unparseable_media() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mkv"), b"not a real container").unwrap();
        dir
    }

    fn tmp_filters() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                r"(/private)?/var/folders/[^/]+/[^/]+/T/\.tmp[A-Za-z0-9]+",
                "[tmp]",
            ),
            (r"/tmp/\.tmp[A-Za-z0-9]+", "[tmp]"),
        ]
    }

    #[rstest]
    fn unparseable_file_still_yields_meta_with_path(
        unparseable_media: tempfile::TempDir,
    ) {
        let path = unparseable_media.path().join("clip.mkv");
        let meta = read_track(&path).unwrap();
        insta::with_settings!({ filters => tmp_filters() }, {
            insta::assert_debug_snapshot!(meta);
        });
    }

    #[rstest]
    fn unparseable_file_has_no_replay_gain(unparseable_media: tempfile::TempDir) {
        let path = unparseable_media.path().join("clip.mkv");
        let meta = read_track(&path).unwrap();
        assert_eq!(meta.audio_format().replay_gain, None);
    }

    #[rstest]
    #[case::negative_with_space("-6.48 dB", Some(-6.48))]
    #[case::negative_no_space("-6.48dB", Some(-6.48))]
    #[case::positive_uppercase("3.2 DB", Some(3.2))]
    #[case::zero_lowercase("0.00 dB", Some(0.0))]
    #[case::mixed_case("-6.48 Db", Some(-6.48))]
    #[case::empty("", None)]
    #[case::unit_only("dB", None)]
    #[case::non_numeric("not a number dB", None)]
    fn replay_gain_db_parsing(#[case] raw: &str, #[case] expected: Option<f32>) {
        assert_eq!(parse_replay_gain(raw), expected);
    }

    #[test]
    fn embedded_cover_of_a_nonexistent_file_is_none() {
        let result = embedded_cover(std::path::Path::new("/nonexistent.mp3"));
        assert_eq!(result, None);
    }

    fn minimal_flac_with_cover(picture_data: &[u8]) -> Vec<u8> {
        let mut mime = Vec::new();
        mime.extend_from_slice(b"image/jpeg");
        let mut description = Vec::new();
        description.extend_from_slice(b"");

        let mut picture_payload = Vec::new();
        picture_payload.extend_from_slice(&3u32.to_be_bytes());
        picture_payload
            .extend_from_slice(&u32::try_from(mime.len()).unwrap_or(0).to_be_bytes());
        picture_payload.extend_from_slice(&mime);
        picture_payload.extend_from_slice(
            &u32::try_from(description.len()).unwrap_or(0).to_be_bytes(),
        );
        picture_payload.extend_from_slice(&description);
        picture_payload.extend_from_slice(&1u32.to_be_bytes());
        picture_payload.extend_from_slice(&1u32.to_be_bytes());
        picture_payload.extend_from_slice(&24u32.to_be_bytes());
        picture_payload.extend_from_slice(&0u32.to_be_bytes());
        picture_payload.extend_from_slice(
            &u32::try_from(picture_data.len()).unwrap_or(0).to_be_bytes(),
        );
        picture_payload.extend_from_slice(picture_data);

        let streaminfo_bits: u64 = (44100u64 << 44) | (1u64 << 41) | (15u64 << 36);
        let mut streaminfo = Vec::new();
        streaminfo.extend_from_slice(&4096u16.to_be_bytes());
        streaminfo.extend_from_slice(&4096u16.to_be_bytes());
        streaminfo.extend_from_slice(&[0, 0, 0]);
        streaminfo.extend_from_slice(&[0, 0, 0]);
        streaminfo.extend_from_slice(&streaminfo_bits.to_be_bytes());
        streaminfo.extend_from_slice(&[0u8; 16]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"fLaC");
        bytes.push(0x00);
        bytes.extend_from_slice(&[0x00, 0x00, 0x22]);
        bytes.extend_from_slice(&streaminfo);
        bytes.push(0x86);
        let payload_len = picture_payload.len().to_be_bytes();
        bytes.extend_from_slice(&payload_len[payload_len.len() - 3..]);
        bytes.extend_from_slice(&picture_payload);
        bytes
    }

    #[test]
    fn embedded_cover_of_a_tagged_file_returns_the_picture_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cover.flac");
        std::fs::write(&path, minimal_flac_with_cover(b"cover-bytes")).unwrap();

        let result = embedded_cover(&path);

        assert_eq!(result, Some(b"cover-bytes".to_vec()));
    }
}
