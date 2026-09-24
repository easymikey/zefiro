use std::sync::Arc;

use strum::{EnumIter, IntoStaticStr};

use crate::domain::{Cursor, CursorDirection, PlaylistIndex, Track};

const ILLEGAL_NAME_CHARS: [char; 9] = ['/', '\\', '?', '<', '>', ':', '*', '|', '"'];

const MAX_NAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistFileName(String);

impl PlaylistFileName {
    pub fn new(name: &str) -> Result<Self, PlaylistNameRejection> {
        let filtered: String = name
            .chars()
            .filter(|ch| !ILLEGAL_NAME_CHARS.contains(ch) && !ch.is_control())
            .collect();
        if filtered.is_empty() {
            return Err(PlaylistNameRejection::Empty);
        }
        if filtered.chars().all(|ch| ch == '.') {
            return Err(PlaylistNameRejection::AllDots);
        }
        Ok(Self(truncated_to_bytes(&filtered, MAX_NAME_BYTES)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
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
pub enum PlaylistNameRejection {
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
    Shuffle(Vec<usize>),
}

impl PlayOrder {
    #[must_use]
    pub fn is_shuffle(&self) -> bool {
        match self {
            PlayOrder::Linear => false,
            PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => true,
        }
    }

    fn order(&self) -> Option<&[usize]> {
        match self {
            PlayOrder::Linear | PlayOrder::ShufflePending => None,
            PlayOrder::Shuffle(order) if order.is_empty() => None,
            PlayOrder::Shuffle(order) => Some(order),
        }
    }

    pub(crate) fn without_order(self) -> Self {
        match self {
            PlayOrder::Linear => PlayOrder::Linear,
            PlayOrder::ShufflePending | PlayOrder::Shuffle(_) => {
                PlayOrder::ShufflePending
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Playlist {
    pub tracks: Vec<Arc<Track>>,
    pub at: Cursor,
    pub play_order: PlayOrder,
    pub repeat: RepeatMode,
}

impl Playlist {
    #[must_use]
    pub fn current(&self) -> Option<&Arc<Track>> {
        self.at.get(&self.tracks)
    }

    #[must_use]
    pub fn anchor(&self) -> Option<PlaylistIndex> {
        (!self.tracks.is_empty()).then(|| PlaylistIndex::new(self.at.index()))
    }

    #[must_use]
    pub fn from_tracks(tracks: Vec<Arc<Track>>) -> Self {
        Self {
            at: Cursor::new(tracks.len()),
            tracks,
            ..Default::default()
        }
    }
}

pub(crate) fn relist(
    playlist: &mut Playlist,
    tracks: Vec<Arc<Track>>,
    anchor: Option<PlaylistIndex>,
) {
    let index = anchor.map_or_else(|| playlist.at.index(), PlaylistIndex::get);
    playlist.at = Cursor::with_len(tracks.len()).at(index);
    playlist.tracks = tracks;
}

fn direction_delta(direction: CursorDirection) -> isize {
    match direction {
        CursorDirection::Forward => 1,
        CursorDirection::Backward => -1,
    }
}

pub fn skip(
    playlist: &mut Playlist,
    direction: CursorDirection,
) -> Option<&Arc<Track>> {
    let next_index = match playlist.play_order.order() {
        Some(_) => skip_shuffled(playlist, direction)?,
        None => skip_linear(playlist, direction)?,
    };
    playlist.at = Cursor::with_len(playlist.tracks.len()).at(next_index);
    playlist.current()
}

fn skip_linear(playlist: &Playlist, direction: CursorDirection) -> Option<usize> {
    if playlist.at.is_empty() {
        return None;
    }
    let len = isize::try_from(playlist.at.len()).ok()?;
    let current = isize::try_from(playlist.at.index()).ok()?;
    let delta = direction_delta(direction);
    match playlist.repeat {
        RepeatMode::All => usize::try_from((current + delta).rem_euclid(len)).ok(),
        RepeatMode::Off | RepeatMode::One => {
            let next = current + delta;
            (next >= 0 && next < len)
                .then_some(next)
                .and_then(|next| usize::try_from(next).ok())
        }
    }
}

fn skip_shuffled(playlist: &Playlist, direction: CursorDirection) -> Option<usize> {
    if playlist.at.is_empty() {
        return None;
    }
    let order = playlist.play_order.order()?;
    let len = isize::try_from(order.len()).ok()?;
    let current = playlist.at.index();
    let position = order
        .iter()
        .position(|&track_index| track_index == current)
        .unwrap_or(0);
    let position = isize::try_from(position).ok()?;
    let delta = direction_delta(direction);
    let wrapped = usize::try_from((position + delta).rem_euclid(len)).ok()?;
    order.get(wrapped).copied()
}

#[must_use]
pub(crate) fn upcoming(playlist: &Playlist) -> Option<&Arc<Track>> {
    let next_index = match playlist.play_order.order() {
        Some(_) => skip_shuffled(playlist, CursorDirection::Forward)?,
        None => skip_linear(playlist, CursorDirection::Forward)?,
    };
    playlist.tracks.get(next_index)
}

pub fn jump(playlist: &mut Playlist, index: PlaylistIndex) -> Option<&Arc<Track>> {
    if index.get() >= playlist.tracks.len() {
        return None;
    }
    playlist.at = Cursor::with_len(playlist.tracks.len()).at(index.get());
    playlist.current()
}

pub(crate) fn anchor_of(
    playing_path: Option<&std::path::Path>,
    tracks: &[Arc<Track>],
) -> Option<PlaylistIndex> {
    let path = playing_path?;
    tracks
        .iter()
        .position(|track| track.path() == path)
        .map(PlaylistIndex::new)
}

#[cfg(test)]
mod playlist_file_name_tests {
    use rstest::rstest;

    use crate::domain::playlist::{PlaylistFileName, PlaylistNameRejection};

    #[rstest]
    #[case::empty("", PlaylistNameRejection::Empty)]
    #[case::whitespace_only_control_chars("\u{0}\u{1}", PlaylistNameRejection::Empty)]
    #[case::only_illegal_characters("???", PlaylistNameRejection::Empty)]
    #[case::all_dots("...", PlaylistNameRejection::AllDots)]
    #[case::single_dot(".", PlaylistNameRejection::AllDots)]
    fn rejects(#[case] input: &str, #[case] expected: PlaylistNameRejection) {
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

    #[test]
    fn into_inner_returns_the_owned_string() {
        let name = PlaylistFileName::new("mix").unwrap();
        assert_eq!(name.into_inner(), "mix".to_string());
    }
}
