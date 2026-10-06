use std::{borrow::Cow, path::Path, sync::Arc};

use kernel::{
    domain::track::{Decibels, Hertz, Kbps, Track, TrackParts},
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
        decibels: tag
            .and_then(|tag| tag.get_string(ItemKey::ReplayGainTrackGain))
            .and_then(parse_decibels),
    };
    let tags = tag.map_or_else(kernel::domain::track::Tags::default, tags_from);
    Ok(Track::new(TrackParts {
        path: path.into(),
        duration,
        tags,
        audio_format,
    }))
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
        track_number: tag.track(),
        track_total: tag.track_total(),
        disc: tag.disk(),
    }
}

fn main_tag(tagged: &TaggedFile) -> Option<&Tag> {
    tagged.primary_tag().or_else(|| tagged.first_tag())
}

fn parse_decibels(tag_text: &str) -> Option<Decibels> {
    tag_text
        .trim()
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
        tags::{embedded_cover, parse_decibels, read_or_list},
        test_support::{minimal_flac_with_cover, temp_dir_filters},
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
    fn unparseable_file_has_no_decibels(unparseable_media: tempfile::TempDir) {
        let path = unparseable_media.path().join("clip.mkv");
        let track = read_or_list(&path);
        assert_eq!(track.audio_format().decibels, None);
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
    fn decibels_read_the_number_before_the_db_unit(
        #[case] tag_text: &str,
        #[case] expected: Option<f32>,
    ) {
        assert_eq!(parse_decibels(tag_text), expected.map(Decibels));
    }

    #[test]
    fn embedded_cover_of_a_nonexistent_file_is_an_error() {
        let result = embedded_cover(std::path::Path::new("/nonexistent.mp3"));
        assert!(result.is_err(), "{result:?}");
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
