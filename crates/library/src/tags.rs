use std::{borrow::Cow, path::Path};

use kernel::{
    domain::track::{AudioFormat, Decibels, Hertz, Kbps, Tags, Track, TrackParts},
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
    let audio_format = AudioFormat {
        format: Some(format!("{:?}", tagged.file_type())),
        sample_rate: properties.sample_rate().map(Hertz),
        bitrate: properties.audio_bitrate().map(Kbps),
        bits_per_sample: properties.bit_depth(),
        channels: properties.channels(),
        decibels: tag
            .and_then(|tag| tag.get_string(ItemKey::ReplayGainTrackGain))
            .and_then(parse_decibels),
    };
    let tags = tag.map_or_else(Tags::default, tags_from);
    Ok(Track::new(TrackParts {
        path: path.into(),
        duration,
        tags,
        audio_format,
    }))
}

fn tags_from(tag: &Tag) -> Tags {
    Tags {
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

const MAX_DECIBELS: f32 = 60.0;

fn parse_decibels(tag_text: &str) -> Option<Decibels> {
    tag_text
        .trim()
        .trim_end_matches(|ch: char| ch.is_ascii_alphabetic())
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|decibels| decibels.abs() <= MAX_DECIBELS)
        .map(Decibels)
}

pub fn embedded_cover(path: &Path) -> Result<Option<Vec<u8>>, FileParseError> {
    let mut tagged = Probe::open(path)?.read()?;
    let tag_type = main_tag(&tagged).map(Tag::tag_type);
    Ok(tag_type
        .and_then(|tag_type| tagged.remove(tag_type))
        .filter(|tag| !tag.pictures().is_empty())
        .map(|mut tag| tag.remove_picture(0).into_data()))
}

#[cfg(test)]
mod tests {
    use kernel::domain::track::Decibels;
    use rstest::rstest;

    use crate::tags::{embedded_cover, parse_decibels};

    #[rstest]
    #[case::negative_with_space("-6.48 dB", Some(-6.48))]
    #[case::negative_no_space("-6.48dB", Some(-6.48))]
    #[case::positive_uppercase("3.2 DB", Some(3.2))]
    #[case::unit_only("dB", None)]
    #[case::not_a_number("nan dB", None)]
    #[case::at_the_bound("60 dB", Some(60.0))]
    #[case::past_the_bound("-60.5 dB", None)]
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
}
