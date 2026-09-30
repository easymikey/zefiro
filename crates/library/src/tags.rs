use std::{borrow::Cow, path::Path};

use kernel::{LibrarySubject, Track};
use lofty::{
    config::ParseOptions,
    file::TaggedFile,
    prelude::{AudioFile, TaggedFileExt},
    probe::Probe,
    tag::{Accessor, ItemKey, Tag},
};

use crate::error::Error;

fn owned_tag(tag: Option<Cow<'_, str>>) -> Option<String> {
    tag.map(Cow::into_owned)
}

pub(crate) fn read_track(path: &Path) -> Result<Track, Error> {
    let file =
        std::fs::File::open(path).map_err(Error::read(LibrarySubject::Scan, path))?;
    let options = ParseOptions::new().read_cover_art(false);
    let Some(tagged) = Probe::new(std::io::BufReader::new(file))
        .options(options)
        .guess_file_type()
        .ok()
        .and_then(|probe| probe.read().ok())
    else {
        return Ok(Track::listed(path));
    };
    let properties = tagged.properties();
    let duration = properties.duration();
    let tag = main_tag(&tagged);
    let audio_format = kernel::AudioFormat {
        format: Some(format!("{:?}", tagged.file_type())),
        sample_rate_hz: properties.sample_rate(),
        bitrate_kbps: properties.audio_bitrate(),
        bits_per_sample: properties.bit_depth(),
        channels: properties.channels(),
        replay_gain: tag
            .and_then(|tag| tag.get_string(ItemKey::ReplayGainTrackGain))
            .and_then(parse_replay_gain),
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

fn main_tag(tagged: &TaggedFile) -> Option<&Tag> {
    tagged.primary_tag().or_else(|| tagged.first_tag())
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
    let tag = main_tag(&tagged)?;
    let picture = tag.pictures().first()?;
    Some(picture.data().to_vec())
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use crate::{
        tags::{embedded_cover, parse_replay_gain, read_track},
        test_support::tmp_filters,
    };

    #[fixture]
    fn unparseable_media() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mkv"), b"not a real container").unwrap();
        dir
    }

    #[rstest]
    fn an_unparseable_file_still_yields_meta_carrying_its_path(
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

    #[test]
    fn embedded_cover_of_a_tagged_file_returns_the_picture_data() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cover.flac");
        std::fs::write(&path, minimal_flac_with_cover(b"cover-bytes")).unwrap();

        let result = embedded_cover(&path);

        assert_eq!(result, Some(b"cover-bytes".to_vec()));
    }
}
