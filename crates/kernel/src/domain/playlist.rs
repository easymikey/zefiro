use std::sync::Arc;

use strum::{EnumIter, IntoStaticStr};

use crate::domain::{
    cursor::Cursor,
    direction::Direction,
    index::ViewIndex,
    track::{Track, TrackSource},
};

const ILLEGAL_NAME_CHARS: [char; 9] = ['/', '\\', '?', '<', '>', ':', '*', '|', '"'];

const MAX_NAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistFileName(String);

impl PlaylistFileName {
    pub fn new(name: &str) -> Result<Self, PlaylistFileNameError> {
        let filtered: String = name
            .chars()
            .filter(|ch| !ILLEGAL_NAME_CHARS.contains(ch) && !ch.is_control())
            .collect();
        if filtered.is_empty() {
            return Err(PlaylistFileNameError::Empty);
        }
        if filtered.chars().all(|ch| ch == '.') {
            return Err(PlaylistFileNameError::AllDots);
        }
        Ok(Self(truncated_to_bytes(&filtered, MAX_NAME_BYTES)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn truncated_to_bytes(input: &str, max_bytes: usize) -> String {
    input
        .chars()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaylistSource {
    #[default]
    Library,
    Named,
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
        let next_index = match self.play_order.order() {
            Some(_) => self.skip_shuffled(direction)?,
            None => self.skip_linear(direction)?,
        };
        self.cursor = Cursor::at(self.tracks.len(), next_index);
        self.current()
    }

    fn skip_linear(&self, direction: Direction) -> Option<usize> {
        if self.cursor.is_empty() {
            return None;
        }
        let len = isize::try_from(self.cursor.len()).ok()?;
        let current = isize::try_from(self.cursor.index()).ok()?;
        let delta = direction.sign();
        match self.repeat_mode {
            RepeatMode::All => usize::try_from((current + delta).rem_euclid(len)).ok(),
            RepeatMode::Off | RepeatMode::One => {
                let next = current + delta;
                (next >= 0 && next < len)
                    .then_some(next)
                    .and_then(|next| usize::try_from(next).ok())
            }
        }
    }

    fn skip_shuffled(&self, direction: Direction) -> Option<usize> {
        if self.cursor.is_empty() {
            return None;
        }
        let order = self.play_order.order()?;
        let current = self.cursor.index();
        let position = order
            .iter()
            .position(|track_index| track_index.get() == current)
            .unwrap_or(0);
        order
            .get(direction.wrapped(position, order.len()))
            .map(|index| index.get())
    }

    #[must_use]
    pub(crate) fn upcoming(&self) -> Option<&Arc<Track>> {
        let next_index = match self.play_order.order() {
            Some(_) => self.skip_shuffled(Direction::Next)?,
            None => self.skip_linear(Direction::Next)?,
        };
        self.tracks.get(next_index)
    }

    pub fn jump(&mut self, index: ViewIndex) -> Option<&Arc<Track>> {
        if index.get() >= self.tracks.len() {
            return None;
        }
        self.cursor = Cursor::at(self.tracks.len(), index.get());
        self.current()
    }
}

pub(crate) fn index_of(tracks: &[Arc<Track>], source: &TrackSource) -> Option<usize> {
    tracks.iter().position(|track| track.source() == source)
}

#[cfg(test)]
mod playlist_file_name_tests {
    use rstest::rstest;

    use crate::domain::playlist::{PlaylistFileName, PlaylistFileNameError};

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
        assert_eq!(name.as_str().len(), 255);
    }
}
