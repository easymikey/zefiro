use std::sync::Arc;

use strum::{EnumIter, IntoStaticStr};

use crate::domain::{
    cursor::Cursor,
    direction::Direction,
    index::ViewIndex,
    server::ServerName,
    track::{Track, TrackSource},
};

const ILLEGAL_NAME_CHARS: [char; 9] = ['/', '\\', '?', '<', '>', ':', '*', '|', '"'];

pub const PLAYLIST_EXTENSION: &str = ".m3u8";

const MAX_NAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistFileName(String);

impl PlaylistFileName {
    pub fn new(name: &str) -> Result<Self, PlaylistFileNameError> {
        let stem = truncated_to_bytes(
            name.chars()
                .filter(|ch| !ILLEGAL_NAME_CHARS.contains(ch) && !ch.is_control()),
            MAX_NAME_BYTES - PLAYLIST_EXTENSION.len(),
        );
        if stem.is_empty() {
            return Err(PlaylistFileNameError::Empty);
        }
        if stem.chars().all(|ch| ch == '.') {
            return Err(PlaylistFileNameError::AllDots);
        }
        Ok(Self(stem))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn truncated_to_bytes(chars: impl Iterator<Item = char>, max_bytes: usize) -> String {
    chars
        .scan(0usize, |bytes, ch| {
            *bytes += ch.len_utf8();
            (*bytes <= max_bytes).then_some(ch)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlaylistFileNameError {
    #[error("enter a playlist name")]
    Empty,
    #[error("a playlist name cannot be only dots")]
    AllDots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PlaylistSource {
    #[default]
    Library,
    Named,
    Server(ServerName),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PlayOrder {
    #[default]
    Linear,
    ShufflePending,
    Shuffled(Vec<ViewIndex>),
}

impl PlayOrder {
    #[must_use]
    pub fn is_shuffle(&self) -> bool {
        match self {
            PlayOrder::Linear => false,
            PlayOrder::ShufflePending | PlayOrder::Shuffled(_) => true,
        }
    }

    fn order(&self) -> Option<&[ViewIndex]> {
        match self {
            PlayOrder::Linear | PlayOrder::ShufflePending => None,
            PlayOrder::Shuffled(order) if order.is_empty() => None,
            PlayOrder::Shuffled(order) => Some(order),
        }
    }

    pub(crate) fn without_order(self) -> Self {
        match self {
            PlayOrder::Linear => PlayOrder::Linear,
            PlayOrder::ShufflePending | PlayOrder::Shuffled(_) => {
                PlayOrder::ShufflePending
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playlist {
    pub tracks: Vec<Arc<Track>>,
    pub cursor: Cursor,
    pub play_order: PlayOrder,
    pub repeat_mode: RepeatMode,
}

impl Playlist {
    #[must_use]
    pub fn current(&self) -> Option<&Arc<Track>> {
        self.cursor.get(&self.tracks)
    }

    #[must_use]
    pub fn playing_index(&self) -> Option<ViewIndex> {
        (!self.tracks.is_empty()).then(|| ViewIndex::new(self.cursor.index()))
    }

    #[must_use]
    pub fn from_tracks(tracks: Vec<Arc<Track>>) -> Self {
        Self {
            cursor: Cursor::new(tracks.len()),
            tracks,
            ..Default::default()
        }
    }

    pub(crate) fn relist(
        &mut self,
        tracks: Vec<Arc<Track>>,
        anchor_index: Option<ViewIndex>,
    ) {
        let index = anchor_index.map_or_else(|| self.cursor.index(), ViewIndex::get);
        self.cursor = Cursor::at(tracks.len(), index);
        self.tracks = tracks;
    }

    pub fn skip(&mut self, direction: Direction) -> Option<&Arc<Track>> {
        let next_index = self.next_index(direction)?;
        self.cursor = Cursor::at(self.tracks.len(), next_index);
        self.current()
    }

    pub(crate) fn next_index(&self, direction: Direction) -> Option<usize> {
        if self.cursor.is_empty() {
            return None;
        }
        let current = self.cursor.index();
        let len = self.cursor.len();
        self.play_order.order().map_or_else(
            || match self.repeat_mode {
                RepeatMode::All => Some(direction.wrapped(current, len)),
                RepeatMode::Off | RepeatMode::One => current
                    .checked_add_signed(direction.sign())
                    .filter(|&next| next < len),
            },
            |order| {
                let position = order
                    .iter()
                    .position(|track_index| track_index.get() == current)
                    .unwrap_or(0);
                order
                    .get(direction.wrapped(position, order.len()))
                    .map(|index| index.get())
            },
        )
    }

    #[must_use]
    pub(crate) fn upcoming(&self) -> Option<&Arc<Track>> {
        self.tracks.get(self.next_index(Direction::Next)?)
    }

    pub fn jump(&mut self, index: ViewIndex) -> Option<&Arc<Track>> {
        if index.get() >= self.tracks.len() {
            return None;
        }
        self.point_at(index);
        self.current()
    }

    pub(crate) fn point_at(&mut self, index: ViewIndex) {
        self.cursor = Cursor::at(self.tracks.len(), index.get());
    }
}

pub(crate) fn index_of(tracks: &[Arc<Track>], source: &TrackSource) -> Option<usize> {
    tracks.iter().position(|track| track.source() == source)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::playlist::{
        PLAYLIST_EXTENSION,
        PlaylistFileName,
        PlaylistFileNameError,
    };

    #[rstest]
    #[case::empty("", PlaylistFileNameError::Empty)]
    #[case::whitespace_only_control_chars("\u{0}\u{1}", PlaylistFileNameError::Empty)]
    #[case::only_illegal_characters("???", PlaylistFileNameError::Empty)]
    #[case::all_dots("...", PlaylistFileNameError::AllDots)]
    #[case::single_dot(".", PlaylistFileNameError::AllDots)]
    fn rejects(#[case] input: &str, #[case] expected: PlaylistFileNameError) {
        assert_eq!(PlaylistFileName::new(input), Err(expected));
    }

    #[test]
    fn keeps_a_plain_name() {
        let name = PlaylistFileName::new("My Mix").unwrap();
        assert_eq!(name.as_str(), "My Mix");
    }

    #[test]
    fn strips_path_and_reserved_separators() {
        let name = PlaylistFileName::new("a/b:c").unwrap();
        assert_eq!(name.as_str(), "abc");
    }

    #[test]
    fn truncates_to_the_byte_budget() {
        let long = "a".repeat(300);
        let name = PlaylistFileName::new(&long).unwrap();
        assert_eq!(name.as_str().len(), 250);
    }

    #[test]
    fn a_max_length_name_still_fits_a_file_name_with_its_extension() {
        let long = "a".repeat(300);
        let name = PlaylistFileName::new(&long).unwrap();
        assert!(format!("{}{PLAYLIST_EXTENSION}", name.as_str()).len() <= 255);
    }

    #[test]
    fn truncation_to_only_dots_is_refused() {
        let dots_then_letter = format!("{}x", ".".repeat(256));
        assert_eq!(
            PlaylistFileName::new(&dots_then_letter),
            Err(PlaylistFileNameError::AllDots)
        );
    }
}
