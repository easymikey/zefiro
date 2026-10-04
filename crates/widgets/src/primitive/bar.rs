use std::{borrow::Cow, time::Duration};

use kernel::domain::{
    appearance::Rgb,
    geometry::{Cells, Pixels},
    player::Player,
    time::Moment,
};
use num_traits::ToPrimitive;
use ratatui::{
    style::Color,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    pixels::numeric::floor,
    primitive::{
        chip::{self, ChipStyle},
        glyphs,
        relative_time::format_time,
        span::{line, text},
    },
    repaint::{ProgressScale, next_progress_step},
    theme::active_theme::{ActiveTheme, ProgressStyle},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBar {
    pub height: Pixels,
    pub radius: Option<Pixels>,
    pub fill: Option<Rgb>,
    pub groove: Option<Rgb>,
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self {
            height: Pixels(4),
            radius: None,
            fill: None,
            groove: None,
        }
    }
}

const FULL_RUN: &str = "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━";
const EMPTY_RUN: &str = "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────";
const VOLUME_RUN: &str = "████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████████";

#[derive(Debug)]
pub(crate) struct BarFill {
    fraction: f32,
    width: usize,
    filled_glyph: &'static str,
    filled_run: &'static str,
    partial: Option<&'static str>,
    groove_glyph: &'static str,
    groove_run: &'static str,
}

impl BarFill {
    #[must_use]
    pub(crate) fn progress(fraction: f32, width: usize) -> Self {
        Self {
            fraction,
            width,
            filled_glyph: glyphs::progress_line::FULL,
            filled_run: FULL_RUN,
            partial: Some(glyphs::progress_line::PARTIAL),
            groove_glyph: glyphs::progress_line::EMPTY,
            groove_run: EMPTY_RUN,
        }
    }

    #[must_use]
    pub(crate) fn volume(fraction: f32, width: usize) -> Self {
        Self {
            fraction,
            width,
            filled_glyph: glyphs::VOLUME_BLOCK,
            filled_run: VOLUME_RUN,
            partial: None,
            groove_glyph: glyphs::VOLUME_BLOCK,
            groove_run: VOLUME_RUN,
        }
    }
}

fn repeat_glyph(
    glyph: &'static str,
    full_run: &'static str,
    cells: usize,
) -> Cow<'static, str> {
    full_run
        .get(..cells.saturating_mul(glyph.len()))
        .map_or_else(|| Cow::Owned(glyph.repeat(cells)), Cow::Borrowed)
}

#[must_use]
pub(crate) fn fill(spec: &BarFill, fill: Color, track: Color) -> Line<'static> {
    let width_f32 = spec.width.to_f32().unwrap_or(f32::MAX);
    let exact = spec.fraction.clamp(0.0, 1.0) * width_f32;
    let whole = floor::<usize>(exact);
    let whole_f32 = whole.to_f32().unwrap_or(f32::MAX);
    let rounds_up = exact - whole_f32 >= 0.5 && whole < spec.width;
    let used = whole + usize::from(rounds_up);
    let partial = rounds_up.then_some(spec.partial).flatten();
    let solid = used.saturating_sub(usize::from(partial.is_some()));
    let filled = repeat_glyph(spec.filled_glyph, spec.filled_run, solid);
    let groove = repeat_glyph(
        spec.groove_glyph,
        spec.groove_run,
        spec.width.saturating_sub(used),
    );
    line(
        [
            Some(text(filled).fg(fill)),
            partial.map(|glyph| text(glyph).fg(fill)),
            Some(text(groove).fg(track)),
        ]
        .into_iter()
        .flatten(),
    )
}

const REMAINING_SIGN: char = '-';
const HUD_GAP: &str = "  ";

#[must_use]
pub(crate) fn remaining_label(remaining: Duration) -> String {
    format!("{REMAINING_SIGN}{}", format_time(remaining))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgressRow {
    pub(crate) fraction: f32,
    pub(crate) row_width: usize,
    pub(crate) remaining: Duration,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HudProgressStyle {
    pub(crate) bar: ProgressStyle,
    pub(crate) chip: ChipStyle,
}

impl HudProgressStyle {
    #[must_use]
    pub(crate) fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        Self {
            bar: ProgressStyle::from_theme(theme),
            chip: ChipStyle::from_theme(theme),
        }
    }
}

#[must_use]
pub(crate) fn hud_progress_bar_width(row_width: u16, remaining: Duration) -> u16 {
    let gap = u16::try_from(HUD_GAP.width()).unwrap_or(u16::MAX);
    let reserved = chip::width(&remaining_label(remaining)).saturating_add(gap);
    if row_width <= reserved {
        row_width
    } else {
        row_width - reserved
    }
}

#[must_use]
pub(crate) fn hud_progress_line(
    input: &HudProgressRow,
    colors: &HudProgressStyle,
) -> Line<'static> {
    let chip_spans = chip::spans(&remaining_label(input.remaining), colors.chip);
    let row_width = u16::try_from(input.row_width).unwrap_or(u16::MAX);
    let bar_width = usize::from(hud_progress_bar_width(row_width, input.remaining));
    if bar_width == input.row_width {
        return fill(
            &BarFill::progress(input.fraction, input.row_width),
            colors.bar.fill,
            colors.bar.track,
        );
    }
    let bar = fill(
        &BarFill::progress(input.fraction, bar_width),
        colors.bar.fill,
        colors.bar.track,
    );
    Line::from_iter(
        bar.spans
            .into_iter()
            .chain([Span::raw(HUD_GAP)])
            .chain(chip_spans),
    )
}

#[must_use]
pub fn progress_frame_due(
    player: &Player,
    bar_width: Option<Cells>,
    now: Moment,
) -> Option<Moment> {
    let Player::Playing { head, track, .. } = player else {
        return None;
    };
    let scale = ProgressScale::text_bar(bar_width?, track.duration()?)?;
    next_progress_step(scale, *head, now)
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use kernel::domain::{
        bounded::Bounded,
        geometry::Cells,
        player::{PausedBy, Player, Preload},
        playhead::Playhead,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track},
    };
    use ratatui::{style::Color, symbols::block, text::Line};
    use rstest::rstest;

    use crate::{
        primitive::{
            bar::{
                BarFill,
                HudProgressRow,
                HudProgressStyle,
                fill,
                hud_progress_line,
                progress_frame_due,
            },
            chip::ChipStyle,
            glyphs,
        },
        repaint::{ProgressScale, next_progress_step},
        theme::active_theme::ProgressStyle,
    };

    fn track(duration: Duration) -> Arc<Track> {
        Arc::new(
            Track::builder()
                .path("/music/song.mp3")
                .duration(duration)
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build(),
        )
    }

    fn playing(offset: Duration, since: Moment, duration: Duration) -> Player {
        Player::Playing {
            track: track(duration),
            head: Playhead::anchored(offset, since, Speed::clamped(1.0)),
            preload: Preload::None,
        }
    }

    fn paused(at: Duration, duration: Duration) -> Player {
        Player::Paused {
            track: track(duration),
            at,
            by: PausedBy::Listener,
        }
    }

    fn painted(spec: &BarFill) -> Line<'static> {
        fill(spec, Color::Green, Color::Black)
    }

    fn progress_text(fraction: f32, width: usize) -> String {
        painted(&BarFill::progress(fraction, width))
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[rstest]
    #[case::empty(0.0, 10)]
    #[case::half(0.5, 10)]
    #[case::just_short_of_full(0.95, 10)]
    #[case::full(1.0, 10)]
    fn the_progress_line_is_box_drawing_with_at_most_one_partial_cell(
        #[case] fraction: f32,
        #[case] width: usize,
    ) {
        let text = progress_text(fraction, width);
        for banned in [
            block::ONE_EIGHTH,
            block::ONE_QUARTER,
            block::THREE_EIGHTHS,
            block::HALF,
            block::FIVE_EIGHTHS,
            block::THREE_QUARTERS,
            block::SEVEN_EIGHTHS,
            block::FULL,
        ] {
            assert!(
                !text.contains(banned),
                "emitted banned block glyph {banned:?}"
            );
        }
        assert!(
            text.matches(glyphs::progress_line::PARTIAL).count() <= 1,
            "at most one partial cell"
        );
        assert_eq!(text.chars().count(), width);
        insta::with_settings!({ snapshot_suffix => format!("{fraction}") }, {
            insta::assert_snapshot!(text);
        });
    }

    fn hud_progress_text(
        fraction: f32,
        row_width: usize,
        remaining: Duration,
    ) -> String {
        let line = hud_progress_line(
            &HudProgressRow {
                fraction,
                row_width,
                remaining,
            },
            &HudProgressStyle {
                bar: ProgressStyle {
                    fill: Color::Red,
                    track: Color::Black,
                },
                chip: ChipStyle {
                    border: Color::Black,
                    foreground: Color::White,
                },
            },
        );
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    struct BarRow {
        name: &'static str,
        fraction: f32,
        row_width: usize,
        remaining_secs: u64,
    }

    #[rstest]
    #[case::at_the_start(BarRow { name: "start", fraction: 0.0, row_width: 30, remaining_secs: 105 })]
    #[case::half_way(BarRow { name: "half", fraction: 0.5, row_width: 30, remaining_secs: 30 })]
    #[case::at_the_end(BarRow { name: "end", fraction: 1.0, row_width: 30, remaining_secs: 0 })]
    #[case::too_narrow_for_a_chip(BarRow { name: "narrow", fraction: 0.5, row_width: 4, remaining_secs: 45 })]
    fn the_hud_row_paints_a_bar_and_its_remaining_chip(#[case] row: BarRow) {
        let text = hud_progress_text(
            row.fraction,
            row.row_width,
            Duration::from_secs(row.remaining_secs),
        );
        assert_eq!(
            text.chars().count(),
            row.row_width,
            "the row fills its width"
        );
        insta::with_settings!({ snapshot_suffix => row.name }, {
            insta::assert_snapshot!(text);
        });
    }

    fn volume_text(fraction: f32, bar_width: usize) -> String {
        painted(&BarFill::volume(fraction, bar_width))
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    #[rstest]
    #[case::silent(0.0)]
    #[case::half(0.5)]
    #[case::full(1.0)]
    fn the_volume_bar_is_solid_blocks_filling_every_row(#[case] fraction: f32) {
        let row = volume_text(fraction, 16);

        assert_eq!(row.chars().count(), 16, "the row fills the bar's width");
        assert_eq!(
            row.matches(glyphs::progress_line::FULL).count(),
            0,
            "the volume bar must not borrow the progress line's glyphs"
        );
        assert_ne!(row, progress_text(fraction, 16));
        assert!(
            row.contains(glyphs::VOLUME_BLOCK) || row.contains(glyphs::VOLUME_BLOCK)
        );
        insta::with_settings!({ snapshot_suffix => format!("{fraction}") }, {
            insta::assert_snapshot!(row);
        });
    }

    #[rstest]
    #[case::silent(0.0, 0)]
    #[case::half(0.5, 8)]
    #[case::full(1.0, 16)]
    fn the_volume_run_is_one_solid_block_that_changes_colour_at_the_level(
        #[case] fraction: f32,
        #[case] filled: usize,
    ) {
        let line = painted(&BarFill::volume(fraction, 16));
        let cells: Vec<(char, Option<Color>)> = line
            .spans
            .iter()
            .flat_map(|span| span.content.chars().map(|glyph| (glyph, span.style.fg)))
            .collect();
        assert_eq!(cells.len(), 16, "the run covers the whole rect");
        let solid = glyphs::VOLUME_BLOCK.chars().next();
        assert!(
            cells.iter().all(|&(glyph, _)| Some(glyph) == solid),
            "every cell of the run is that solid block"
        );
        assert_eq!(
            cells
                .iter()
                .filter(|&&(_, fg)| fg == Some(Color::Green))
                .count(),
            filled,
            "the fill colour reaches exactly the level"
        );
    }

    #[rstest]
    #[case::a_stopped_player_has_no_progress_frame(Player::Stopped, Some(50), None)]
    #[case::a_paused_player_has_no_progress_frame(
        paused(Duration::from_secs(10), Duration::from_secs(100)),
        Some(50),
        None
    )]
    #[case::a_playing_track_wants_the_next_progress_step(
        playing(
            Duration::from_millis(10_200),
            Moment::new(Duration::from_secs(100)),
            Duration::from_secs(100)
        ),
        Some(50),
        Some(Moment::new(Duration::from_millis(100_801)))
    )]
    #[case::no_bar_has_no_progress_frame(
        playing(
            Duration::from_millis(10_200),
            Moment::new(Duration::from_secs(100)),
            Duration::from_secs(100)
        ),
        None,
        None
    )]
    #[case::a_sped_up_track_still_wants_a_progress_step(
        Player::Playing {
            track: track(Duration::from_secs(100)),
            head: Playhead::anchored(
                Duration::from_millis(10_200),
                Moment::new(Duration::from_secs(100)),
                Speed::clamped(1.5)
            ),
            preload: Preload::None,
        },
        Some(50),
        next_progress_step(
            ProgressScale::text_bar(Cells(50), Duration::from_secs(100)).unwrap(),
            Playhead::anchored(
                Duration::from_millis(10_200),
                Moment::new(Duration::from_secs(100)),
                Speed::clamped(1.5)
            ),
            Moment::new(Duration::from_secs(100))
        )
    )]
    fn a_progress_frame_is_due_only_while_the_bar_can_move(
        #[case] player: Player,
        #[case] bar_width: Option<u16>,
        #[case] expected: Option<Moment>,
    ) {
        let now = Moment::new(Duration::from_secs(100));

        assert_eq!(
            progress_frame_due(&player, bar_width.map(Cells), now),
            expected
        );
    }
}
