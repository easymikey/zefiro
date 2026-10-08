use std::{cmp::Reverse, path::Path, sync::Arc};

use crate::domain::{index::ViewIndex, track::Track};

const MATCH_SCORE: i32 = 16;
const CONSECUTIVE_BONUS: i32 = 15;
const BOUNDARY_BONUS: i32 = 8;
const POSITION_PENALTY_DIVISOR: i32 = 8;

#[derive(Default)]
struct Scoring {
    total: i32,
    query_index: usize,
    previous_matched: Option<usize>,
    previous_char: Option<char>,
}

impl Scoring {
    fn advance(
        mut self,
        query_chars: &[char],
        step: (usize, char),
    ) -> Result<Self, Self> {
        let (position, haystack_char) = step;
        let Some(&query_char) = query_chars.get(self.query_index) else {
            return Err(self);
        };
        if haystack_char == query_char {
            self.total += MATCH_SCORE;
            let starts_word = self
                .previous_char
                .is_none_or(|previous| !previous.is_alphanumeric());
            if starts_word {
                self.total += BOUNDARY_BONUS;
            }
            if self
                .previous_matched
                .is_some_and(|previous| previous + 1 == position)
            {
                self.total += CONSECUTIVE_BONUS;
            }
            self.previous_matched = Some(position);
            self.query_index += 1;
        }
        self.previous_char = Some(haystack_char);
        Ok(self)
    }
}

fn score_chars(query_chars: &[char], haystack: &str) -> Option<i32> {
    if query_chars.is_empty() {
        return Some(0);
    }
    let scoring = haystack
        .chars()
        .flat_map(char::to_lowercase)
        .enumerate()
        .try_fold(Scoring::default(), |scoring, step| {
            scoring.advance(query_chars, step)
        })
        .unwrap_or_else(|finished| finished);

    if scoring.query_index < query_chars.len() {
        return None;
    }
    Some(scoring.previous_matched.map_or(scoring.total, |last| {
        let last = i32::try_from(last).unwrap_or(i32::MAX);
        scoring.total - last / POSITION_PENALTY_DIVISOR
    }))
}

#[must_use]
pub fn rank(tracks: &[Arc<Track>], query: &str) -> Vec<ViewIndex> {
    if query.is_empty() {
        return (0..tracks.len()).map(ViewIndex::new).collect();
    }
    ranked(tracks, query, 0..tracks.len())
}

#[must_use]
pub(crate) fn narrow(
    tracks: &[Arc<Track>],
    query: &str,
    matches: &[ViewIndex],
) -> Vec<ViewIndex> {
    if query.is_empty() {
        return rank(tracks, query);
    }
    ranked(tracks, query, matches.iter().map(|index| index.get()))
}

fn ranked(
    tracks: &[Arc<Track>],
    query: &str,
    candidates: impl Iterator<Item = usize>,
) -> Vec<ViewIndex> {
    let query_chars: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let mut scored: Vec<(usize, i32)> = candidates
        .filter_map(|index| {
            let track = tracks.get(index)?;
            best_track_score(&query_chars, track).map(|score| (index, score))
        })
        .collect();
    scored.sort_by_key(|&(index, score)| (Reverse(score), index));
    scored
        .into_iter()
        .map(|(index, _)| ViewIndex::new(index))
        .collect()
}

fn best_track_score(query_chars: &[char], track: &Track) -> Option<i32> {
    let file_name = track
        .local_path()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    [
        track.tags().title.as_deref(),
        track.tags().artist.as_deref(),
        track.tags().album.as_deref(),
        file_name,
    ]
    .into_iter()
    .flatten()
    .filter_map(|field| score_chars(query_chars, field))
    .max()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::search::score_chars;

    fn lower(haystack: &str) -> String {
        haystack.chars().flat_map(char::to_lowercase).collect()
    }

    fn score(query: &str, haystack: &str) -> Option<i32> {
        let query_chars: Vec<char> = lower(query).chars().collect();
        score_chars(&query_chars, haystack)
    }

    #[rstest]
    #[case("ist", "İst", 79)]
    fn a_match_survives_non_ascii_lowercase_expansion(
        #[case] query: &str,
        #[case] haystack: &str,
        #[case] expected: i32,
    ) {
        assert_eq!(score(query, haystack), Some(expected));
    }
}
