use std::{cmp::Reverse, sync::Arc};

use crate::domain::Track;

struct ScoreWeights {
    match_score: i32,
    consecutive_bonus: i32,
    boundary_bonus: i32,
    position_penalty_divisor: i32,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            match_score: 16,
            consecutive_bonus: 15,
            boundary_bonus: 8,
            position_penalty_divisor: 8,
        }
    }
}

#[cfg(test)]
#[must_use]
fn score(query: &str, haystack: &str) -> Option<i32> {
    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    score_chars(&query_chars, haystack)
}

#[derive(Default)]
struct Scoring {
    total: i32,
    query_index: usize,
    previous_matched: Option<usize>,
    previous_char: Option<char>,
}

struct MatchContext<'a> {
    query_chars: &'a [char],
    weights: &'a ScoreWeights,
}

impl Scoring {
    fn advance(
        mut self,
        context: &MatchContext<'_>,
        step: (usize, char),
    ) -> Result<Self, Self> {
        let (position, haystack_char) = step;
        let Some(&query_char) = context.query_chars.get(self.query_index) else {
            return Err(self);
        };
        if haystack_char == query_char {
            self.total += context.weights.match_score;
            let starts_word = self
                .previous_char
                .is_none_or(|previous| !previous.is_alphanumeric());
            if starts_word {
                self.total += context.weights.boundary_bonus;
            }
            if self
                .previous_matched
                .is_some_and(|previous| previous + 1 == position)
            {
                self.total += context.weights.consecutive_bonus;
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
    let weights = ScoreWeights::default();
    let context = MatchContext {
        query_chars,
        weights: &weights,
    };
    let scoring = haystack
        .chars()
        .flat_map(char::to_lowercase)
        .enumerate()
        .try_fold(Scoring::default(), |scoring, step| {
            scoring.advance(&context, step)
        })
        .unwrap_or_else(|finished| finished);

    if scoring.query_index < query_chars.len() {
        return None;
    }
    let mut total = scoring.total;
    if let Some(last) = scoring.previous_matched {
        let last = i32::try_from(last).unwrap_or(i32::MAX);
        total -= last / weights.position_penalty_divisor;
    }
    Some(total)
}

#[must_use]
pub fn rank(tracks: &[Arc<Track>], query: &str) -> Vec<usize> {
    let mut matches = Vec::new();
    rank_into(tracks, query, &mut matches);
    matches
}

pub fn rank_into(tracks: &[Arc<Track>], query: &str, matches: &mut Vec<usize>) {
    matches.clear();
    if query.is_empty() {
        matches.extend(0..tracks.len());
        return;
    }
    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    let mut scored: Vec<(usize, i32)> = tracks
        .iter()
        .enumerate()
        .filter_map(|(index, track)| {
            best_track_score(&query_chars, track).map(|score| (index, score))
        })
        .collect();
    scored.sort_by_key(|&(index, score)| (Reverse(score), index));
    matches.extend(scored.into_iter().map(|(index, _)| index));
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
mod score_tests {
    use std::{collections::BTreeSet, sync::Arc, time::Duration};

    use proptest::prelude::{Strategy, prop_assert, prop_assert_eq, proptest};
    use rstest::rstest;

    use crate::{
        domain::{AudioFormat, Tags, Track},
        search::{rank_into, score},
    };

    fn ascii_lower() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            proptest::sample::select(vec!['m', 'n', 'o', 'p', 'q']),
            0..6,
        )
        .prop_map(|chars| chars.into_iter().collect())
    }

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path(format!("{title}.flac"))
                .duration(Duration::from_secs(1))
                .tags(Tags {
                    title: Some(title.to_string()),
                    ..Tags::default()
                })
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

    fn is_subsequence(needle: &str, haystack: &str) -> bool {
        let mut haystack_chars = haystack.chars();
        needle
            .chars()
            .all(|wanted| haystack_chars.any(|seen| seen == wanted))
    }

    #[test]
    fn none_when_query_chars_are_not_a_subsequence() {
        assert_eq!(score("xyz", "Moon River"), None);
    }

    #[test]
    fn some_when_query_is_a_subsequence_case_insensitively() {
        assert!(score("mnrv", "Moon River").is_some());
        assert!(score("MNRV", "moon river").is_some());
    }

    #[test]
    fn ranks_contiguous_prefix_above_scattered_subsequence() {
        let tight = score("moon", "Moon River").unwrap();
        let scattered = score("mnrv", "Moon River").unwrap();
        assert!(
            tight > scattered,
            "tight={tight} scattered={scattered} (expected tight > scattered)"
        );
    }

    #[rstest]
    #[case("ist", "İst", 79)]
    fn matches_through_expanded_non_ascii_lowercasing(
        #[case] query: &str,
        #[case] haystack: &str,
        #[case] expected: i32,
    ) {
        assert_eq!(score(query, haystack), Some(expected));
    }

    proptest! {
        #[test]
        fn is_some_iff_query_is_a_lowercase_subsequence(
            query in ascii_lower(),
            haystack in ascii_lower(),
        ) {
            let matched = is_subsequence(&query.to_lowercase(), &haystack.to_lowercase());
            prop_assert_eq!(score(&query, &haystack).is_some(), matched);
        }

        #[test]
        fn rank_into_is_a_stable_permutation_of_the_matching_titles(
            titles in proptest::collection::vec(ascii_lower(), 0..8),
            query in ascii_lower(),
        ) {
            let tracks: Vec<Arc<Track>> = titles.iter().map(|title| titled_track(title)).collect();
            let mut matches = Vec::new();
            rank_into(&tracks, &query, &mut matches);

            let expected: BTreeSet<usize> = titles
                .iter()
                .enumerate()
                .filter_map(|(index, title)| score(&query, title).map(|_| index))
                .collect();
            let found: BTreeSet<usize> = matches.iter().copied().collect();
            prop_assert_eq!(found.len(), matches.len());
            prop_assert_eq!(found, expected);

            let scores: Vec<Option<i32>> = titles.iter().map(|title| score(&query, title)).collect();
            let scored = |index: usize| scores.get(index).copied().flatten().unwrap_or(i32::MIN);
            for window in matches.windows(2) {
                if let [left, right] = *window {
                    let (left_score, right_score) = (scored(left), scored(right));
                    prop_assert!(left_score >= right_score);
                    if left_score == right_score {
                        prop_assert!(left < right);
                    }
                }
            }
        }
    }
}
