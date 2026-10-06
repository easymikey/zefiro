use std::{cmp::Reverse, sync::Arc};

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
    let mut matches = Vec::new();
    rank_into(tracks, query, &mut matches);
    matches
}

pub(crate) fn rank_into(
    tracks: &[Arc<Track>],
    query: &str,
    matches: &mut Vec<ViewIndex>,
) {
    matches.clear();
    if query.is_empty() {
        matches.extend((0..tracks.len()).map(ViewIndex::new));
        return;
    }
    *matches = ranked(tracks, query, 0..tracks.len());
}

pub(crate) fn narrow_into(
    tracks: &[Arc<Track>],
    query: &str,
    matches: &mut Vec<ViewIndex>,
) {
    if query.is_empty() {
        rank_into(tracks, query, matches);
        return;
    }
    let narrowed = ranked(tracks, query, matches.iter().map(|index| index.get()));
    *matches = narrowed;
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
    let file_name = track.path().file_name().and_then(|name| name.to_str());
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
    use std::{collections::BTreeSet, sync::Arc, time::Duration};

    use proptest::prelude::{Strategy, prop_assert, prop_assert_eq, proptest};
    use rstest::rstest;

    use crate::{
        domain::{
            index::ViewIndex,
            track::{AudioFormat, Tags, Track, TrackParts},
        },
        search::{narrow_into, rank, score_chars},
    };

    fn lower(haystack: &str) -> String {
        haystack.chars().flat_map(char::to_lowercase).collect()
    }

    fn score(query: &str, haystack: &str) -> Option<i32> {
        let query_chars: Vec<char> = lower(query).chars().collect();
        score_chars(&query_chars, haystack)
    }

    fn chars() -> impl Strategy<Value = char> {
        proptest::sample::select(vec!['m', 'n', 'Α', 'α', 'Σ', 'σ', 'ς', 'İ'])
    }

    fn title() -> impl Strategy<Value = String> {
        proptest::collection::vec(chars(), 0..6)
            .prop_map(|chars| chars.into_iter().collect())
    }

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: format!("{title}.flac").into(),
            duration: Duration::from_secs(1),
            tags: Tags {
                title: Some(title.to_string()),
                ..Tags::default()
            },
            audio_format: AudioFormat::default(),
        }))
    }

    fn is_subsequence(needle: &str, haystack: &str) -> bool {
        let mut haystack_chars = haystack.chars();
        needle
            .chars()
            .all(|wanted| haystack_chars.any(|seen| seen == wanted))
    }

    #[test]
    fn a_query_that_is_not_a_subsequence_scores_none() {
        assert_eq!(score("xyz", "Moon River"), None);
    }

    #[test]
    fn a_query_that_is_a_subsequence_scores_some_ignoring_case() {
        assert!(score("mnrv", "Moon River").is_some());
        assert!(score("MNRV", "moon river").is_some());
    }

    #[test]
    fn a_contiguous_prefix_ranks_above_a_scattered_subsequence() {
        let tight = score("moon", "Moon River").unwrap();
        let scattered = score("mnrv", "Moon River").unwrap();
        assert!(
            tight > scattered,
            "tight={tight} scattered={scattered} (expected tight > scattered)"
        );
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

    #[test]
    fn a_search_for_a_word_final_sigma_finds_its_own_title() {
        let tracks = vec![titled_track("ΑΣ")];
        assert_eq!(rank(&tracks, "ΑΣ"), vec![ViewIndex::new(0)]);
    }

    #[test]
    fn narrowing_a_search_past_a_word_final_sigma_equals_a_full_rank() {
        let tracks = vec![titled_track("ΑΣΑ")];
        let mut narrowed = rank(&tracks, "ΑΣ");
        narrow_into(&tracks, "ΑΣΑ", &mut narrowed);
        assert_eq!(narrowed, rank(&tracks, "ΑΣΑ"));
        assert_eq!(narrowed, vec![ViewIndex::new(0)]);
    }

    proptest! {
        #[test]
        fn is_some_iff_query_is_a_lowercase_subsequence(
            query in title(),
            haystack in title(),
        ) {
            let is_match = is_subsequence(&lower(&query), &lower(&haystack));
            prop_assert_eq!(score(&query, &haystack).is_some(), is_match);
        }

        #[test]
        fn narrowing_by_an_appended_char_equals_a_full_rank(
            titles in proptest::collection::vec(title(), 0..8),
            query in title(),
            appended in chars(),
        ) {
            let tracks: Vec<Arc<Track>> = titles.iter().map(|title| titled_track(title)).collect();
            let longer = format!("{query}{appended}");
            let mut narrowed = rank(&tracks, &query);
            narrow_into(&tracks, &longer, &mut narrowed);
            prop_assert_eq!(narrowed, rank(&tracks, &longer));
        }

        #[test]
        fn rank_into_is_a_stable_permutation_of_the_matching_titles(
            titles in proptest::collection::vec(title(), 0..8),
            query in title(),
        ) {
            let tracks: Vec<Arc<Track>> = titles.iter().map(|title| titled_track(title)).collect();
            let ranked = rank(&tracks, &query);

            let expected: BTreeSet<usize> = titles
                .iter()
                .enumerate()
                .filter_map(|(index, title)| score(&query, title).map(|_| index))
                .collect();
            let found: BTreeSet<usize> = ranked.iter().copied().map(usize::from).collect();
            prop_assert_eq!(found.len(), ranked.len());
            prop_assert_eq!(found, expected);

            let scores: Vec<Option<i32>> = titles.iter().map(|title| score(&query, title)).collect();
            let scored = |index: usize| scores.get(index).copied().flatten().unwrap_or(i32::MIN);
            for window in ranked.windows(2) {
                if let [left, right] = *window {
                    let (left_score, right_score) = (scored(left.get()), scored(right.get()));
                    prop_assert!(left_score >= right_score);
                    if left_score == right_score {
                        prop_assert!(left < right);
                    }
                }
            }
        }
    }
}
