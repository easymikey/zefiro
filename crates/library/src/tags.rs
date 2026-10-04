use std::{borrow::Cow, path::Path, sync::Arc};

use kernel::{
    domain::track::{Decibels, Hertz, Kbps, Track},
    message::LibrarySubject,
};
use lofty::{
    config::ParseOptions,
    error::FileParseError,
    file::TaggedFile,
    prelude::{AudioFile, TaggedFileExt},
    probe::Probe,
    tag::{Accessor, ItemKey, Tag},
};

use crate::error::Error;

fn tag_text(tag: Option<Cow<'_, str>>) -> Option<String> {
    tag.map(Cow::into_owned)
}

pub(crate) fn read_or_list(path: &Path) -> Arc<Track> {
    Arc::new(read_track(path).unwrap_or_else(|_unread| Track::listed(path)))
}

fn probe(path: &Path) -> Result<TaggedFile, Error> {
    let options = ParseOptions::new().read_cover_art(false);
    let file =
        std::fs::File::open(path).map_err(Error::io(LibrarySubject::Scan, path))?;
    Probe::new(std::io::BufReader::new(file))
        .options(options)
        .guess_file_type()
        .map_err(Error::io(LibrarySubject::Scan, path))?
        .read()
        .map_err(|source| Error::Tags {
            path: path.to_path_buf(),
            source,
        })
}

pub(crate) fn read_track(path: &Path) -> Result<Track, Error> {
    let tagged = probe(path)?;
    let properties = tagged.properties();
    let duration = properties.duration();
    let tag = main_tag(&tagged);
    let audio_format = kernel::domain::track::AudioFormat {
        format: Some(format!("{:?}", tagged.file_type())),
        sample_rate: properties.sample_rate().map(Hertz),
        bitrate: properties.audio_bitrate().map(Kbps),
        bits_per_sample: properties.bit_depth(),
        channels: properties.channels(),
        replay_gain: tag
            .and_then(|tag| tag.get_string(ItemKey::ReplayGainTrackGain))
            .and_then(parse_replay_gain),
    };
    let tags = tag.map_or_else(kernel::domain::track::Tags::default, tags_from);
    Ok(Track::builder()
        .path(path)
        .duration(duration)
        .tags(tags)
        .audio_format(audio_format)
        .build())
}

fn tags_from(tag: &Tag) -> kernel::domain::track::Tags {
    kernel::domain::track::Tags {
        title: tag_text(tag.title()),
        artist: tag_text(tag.artist()),
        album: tag_text(tag.album()),
        genre: tag_text(tag.genre()),
        comment: tag_text(tag.comment()),
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

fn parse_replay_gain(raw: &str) -> Option<Decibels> {
    raw.trim()
        .trim_end_matches(|ch: char| ch.is_ascii_alphabetic())
        .trim()
        .parse()
        .ok()
        .map(Decibels)
}

pub fn embedded_cover(path: &Path) -> Result<Option<Vec<u8>>, FileParseError> {
    let tagged = Probe::open(path)?.read()?;
    Ok(main_tag(&tagged)
        .and_then(|tag| tag.pictures().first())
        .map(|picture| picture.data().to_vec()))
}

#[cfg(test)]
mod tests {
    use kernel::domain::track::Decibels;
    use rstest::{fixture, rstest};

    use crate::{
        tags::{embedded_cover, parse_replay_gain, read_or_list},
        test_support::temp_dir_filters,
    };

    #[fixture]
    fn unparseable_media() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("clip.mkv"), b"not a real container").unwrap();
        dir
    }

    #[rstest]
    fn an_unparseable_file_is_still_listed_by_path(
        unparseable_media: tempfile::TempDir,
    ) {
        let path = unparseable_media.path().join("clip.mkv");
        let track = read_or_list(&path);
        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(track);
        });
    }

    #[rstest]
    fn unparseable_file_has_no_replay_gain(unparseable_media: tempfile::TempDir) {
        let path = unparseable_media.path().join("clip.mkv");
        let track = read_or_list(&path);
        assert_eq!(track.audio_format().replay_gain, None);
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
    fn replay_gain_reads_the_number_before_the_db_unit(
        #[case] raw: &str,
        #[case] expected: Option<f32>,
    ) {
        assert_eq!(parse_replay_gain(raw), expected.map(Decibels));
    }

    #[test]
    fn embedded_cover_of_a_nonexistent_file_is_an_error() {
        let result = embedded_cover(std::path::Path::new("/nonexistent.mp3"));
        assert!(result.is_err(), "{result:?}");
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

        assert_eq!(result.unwrap(), Some(b"cover-bytes".to_vec()));
    }
}
