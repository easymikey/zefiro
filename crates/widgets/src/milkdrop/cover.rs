use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kernel::{cmd::Playback, domain::revision::Revision};
use ratatui::{layout::Rect, text::Line};

use crate::{
    card::CardCover,
    milkdrop::{MilkdropAdvance, MilkdropField, MilkdropStyle, lines},
    scene::Scene,
};

const STEP: Duration = Duration::from_millis(33);
const MAX_CATCH_UP_STEPS: u32 = 4;

type MilkdropResetKey = (u64, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MilkdropStamp {
    reset_key: MilkdropResetKey,
    theme_revision: Revision,
    since_first_paint: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MilkdropPlan {
    Rebuild,
    Recolour,
    Advance,
    Reuse,
}

#[must_use]
fn plan_milkdrop(
    installed_stamp: Option<MilkdropStamp>,
    desired_stamp: MilkdropStamp,
    playback: Playback,
) -> MilkdropPlan {
    let Some(installed_stamp) = installed_stamp else {
        return MilkdropPlan::Rebuild;
    };
    if installed_stamp.reset_key != desired_stamp.reset_key
        || desired_stamp.since_first_paint < installed_stamp.since_first_paint
    {
        MilkdropPlan::Rebuild
    } else if installed_stamp.theme_revision != desired_stamp.theme_revision {
        MilkdropPlan::Recolour
    } else if desired_stamp.since_first_paint - installed_stamp.since_first_paint
        >= STEP
        && playback == Playback::Playing
    {
        MilkdropPlan::Advance
    } else {
        MilkdropPlan::Reuse
    }
}

fn milkdrop_seed(path: Option<&Path>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match path {
        Some(path) => path.hash(&mut hasher),
        None => "terminal::milkdrop::no-track".hash(&mut hasher),
    }
    hasher.finish()
}

#[derive(Default, Debug)]
pub struct MilkdropCover {
    installed: Option<(MilkdropField, MilkdropStamp)>,
    track_seed: Option<(PathBuf, u64)>,
    lines: Arc<[Line<'static>]>,
}

impl MilkdropCover {
    fn track_seed_for(&mut self, current: Option<&Path>) -> u64 {
        match (current, &self.track_seed) {
            (Some(path), Some((stored, hash))) if stored == path => *hash,
            (Some(path), _) => {
                let hash = milkdrop_seed(Some(path));
                self.track_seed = Some((path.to_path_buf(), hash));
                hash
            }
            (None, _) => {
                self.track_seed = None;
                milkdrop_seed(None)
            }
        }
    }

    pub fn refresh(
        &mut self,
        scene: &Scene<'_>,
        cover_area: Option<Rect>,
    ) -> CardCover {
        let Some(rect) = cover_area else {
            self.installed = None;
            self.lines = Arc::default();
            return CardCover::Missing;
        };
        let width = usize::from(rect.width);
        let height = usize::from(rect.height);
        let seed = self.track_seed_for(
            scene.player.current().and_then(|track| track.local_path()),
        );
        let desired_stamp = MilkdropStamp {
            reset_key: (seed, width, height),
            theme_revision: scene.revisions.theme,
            since_first_paint: scene.presentation.since_first_paint,
        };
        let playback = Playback::from(scene.player);
        let installed = self.installed.as_ref().map(|(_, stamp)| *stamp);
        let since_first_paint = desired_stamp.since_first_paint;
        let milkdrop_advance = MilkdropAdvance {
            spectrum: scene.presentation.spectrum,
            playback,
            seed,
            tick: tick(since_first_paint),
        };
        match plan_milkdrop(installed, desired_stamp, playback) {
            MilkdropPlan::Rebuild => {
                let mut field = MilkdropField::new(width, height);
                advance(&mut field, &milkdrop_advance, 1);
                self.installed = Some((field, desired_stamp));
                self.repaint(scene);
            }
            MilkdropPlan::Recolour => {
                if let Some((_, installed_stamp)) = self.installed.as_mut() {
                    installed_stamp.theme_revision = desired_stamp.theme_revision;
                }
                self.repaint(scene);
            }
            MilkdropPlan::Advance => {
                if let Some((field, installed_stamp)) = self.installed.as_mut() {
                    let start = installed_stamp.since_first_paint;
                    let whole = elapsed_steps(*installed_stamp, since_first_paint);
                    advance(
                        field,
                        &MilkdropAdvance {
                            tick: tick(start + STEP),
                            ..milkdrop_advance
                        },
                        whole.min(MAX_CATCH_UP_STEPS),
                    );
                    installed_stamp.since_first_paint = if whole > MAX_CATCH_UP_STEPS {
                        since_first_paint
                    } else {
                        start + STEP * whole
                    };
                }
                self.repaint(scene);
            }
            MilkdropPlan::Reuse => {}
        }
        CardCover::Text(Arc::clone(&self.lines))
    }

    fn repaint(&mut self, scene: &Scene<'_>) {
        if let Some((field, _)) = self.installed.as_ref() {
            let style = MilkdropStyle::from_theme(&scene.active_theme());
            self.lines = lines(field, style);
        }
    }
}

fn tick(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn advance(
    field: &mut MilkdropField,
    milkdrop_advance: &MilkdropAdvance<'_>,
    steps: u32,
) {
    for step in 0..u64::from(steps) {
        field.advance(&MilkdropAdvance {
            tick: milkdrop_advance
                .tick
                .saturating_add(tick(STEP).saturating_mul(step)),
            ..*milkdrop_advance
        });
    }
}

fn elapsed_steps(installed_stamp: MilkdropStamp, since_first_paint: Duration) -> u32 {
    let elapsed = since_first_paint.saturating_sub(installed_stamp.since_first_paint);
    u32::try_from(elapsed.as_millis() / STEP.as_millis()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use kernel::{
        cmd::Playback,
        domain::{
            appearance::Rgb,
            player::{PausedBy, Player},
            playhead::Playhead,
            revision::Revision,
            speed::Speed,
            theme::ThemeName,
            time::Moment,
            track::Track,
        },
    };
    use ratatui::{layout::Rect, text::Line};
    use rstest::rstest;

    use crate::{
        card::CardCover,
        milkdrop::cover::{
            MilkdropCover,
            MilkdropPlan,
            MilkdropStamp,
            milkdrop_seed,
            plan_milkdrop,
        },
        test_support::{SceneSources, model_with_tracks, stock_theme},
        theme::colors::ThemeBase,
    };

    fn stamp(reset_key: (u64, usize, usize), millis: u64) -> MilkdropStamp {
        MilkdropStamp {
            reset_key,
            theme_revision: Revision::default(),
            since_first_paint: Duration::from_millis(millis),
        }
    }

    fn themed(milkdrop_stamp: MilkdropStamp) -> MilkdropStamp {
        MilkdropStamp {
            theme_revision: Revision::default().next(),
            ..milkdrop_stamp
        }
    }

    struct PlanRow {
        installed_stamp: Option<MilkdropStamp>,
        desired_stamp: MilkdropStamp,
        playback: Playback,
    }

    #[rstest]
    #[case::same_reset_key_and_clock(PlanRow { installed_stamp: Some(stamp((1, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 100), playback: Playback::Playing }, MilkdropPlan::Reuse)]
    #[case::a_new_clock_advances(PlanRow { installed_stamp: Some(stamp((1, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 133), playback: Playback::Playing }, MilkdropPlan::Advance)]
    #[case::less_than_a_step_reuses(PlanRow { installed_stamp: Some(stamp((1, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 132), playback: Playback::Playing }, MilkdropPlan::Reuse)]
    #[case::a_clock_gone_backwards_rebuilds(PlanRow { installed_stamp: Some(stamp((1, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 50), playback: Playback::Playing }, MilkdropPlan::Rebuild)]
    #[case::paused_clock_movement_reuses(PlanRow { installed_stamp: Some(stamp((1, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 133), playback: Playback::Paused }, MilkdropPlan::Reuse)]
    #[case::paused_reset_key_change_rebuilds(PlanRow { installed_stamp: Some(stamp((2, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 133), playback: Playback::Paused }, MilkdropPlan::Rebuild)]
    #[case::a_different_reset_key_rebuilds(PlanRow { installed_stamp: Some(stamp((2, 20, 8), 100)), desired_stamp: stamp((1, 20, 8), 100), playback: Playback::Playing }, MilkdropPlan::Rebuild)]
    #[case::a_new_theme_recolours_even_when_paused(PlanRow { installed_stamp: Some(stamp((1, 20, 8), 100)), desired_stamp: themed(stamp((1, 20, 8), 100)), playback: Playback::Paused }, MilkdropPlan::Recolour)]
    #[case::nothing_installed_rebuilds(PlanRow { installed_stamp: None, desired_stamp: stamp((1, 20, 8), 100), playback: Playback::Playing }, MilkdropPlan::Rebuild)]
    fn plan_milkdrop_decides_rebuild_advance_or_reuse(
        #[case] plan_row: PlanRow,
        #[case] expected: MilkdropPlan,
    ) {
        let PlanRow {
            installed_stamp,
            desired_stamp,
            playback,
        } = plan_row;
        assert_eq!(
            plan_milkdrop(installed_stamp, desired_stamp, playback),
            expected
        );
    }

    #[test]
    fn different_paths_seed_differently() {
        let a = PathBuf::from("/music/a.flac");
        let b = PathBuf::from("/music/b.flac");
        assert_ne!(milkdrop_seed(Some(&a)), milkdrop_seed(Some(&b)));
    }

    fn switch_to_ember(sources: &mut SceneSources) {
        sources.theme = stock_theme(
            ThemeName::from_static("ember"),
            &ThemeBase {
                background: Rgb([0x12, 0x12, 0x12]),
                muted_foreground: Rgb([0x90, 0x78, 0x68]),
                foreground: Rgb([0xe8, 0xd0, 0xb8]),
                accent: Rgb([0xe0, 0x70, 0x40]),
                green: Rgb([0x9a, 0xaa, 0x68]),
                yellow: Rgb([0xd8, 0xa0, 0x50]),
                red: Rgb([0xd1, 0x5a, 0x5a]),
                window_background: None,
            },
        );
        sources.model.revisions.theme.advance();
    }

    fn text(cover: CardCover) -> Arc<[Line<'static>]> {
        let CardCover::Text(lines) = cover else {
            panic!("milkdrop paints text lines");
        };
        lines
    }

    fn playing(path: &str) -> Player {
        Player::Playing {
            track: Arc::new(Track::listed(Path::new(path))),
            playhead: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        }
    }

    #[test]
    fn a_track_change_reseeds_the_field() {
        let mut sources = SceneSources::new(model_with_tracks(1));
        let area = Some(Rect::new(0, 0, 12, 6));
        let mut cover = MilkdropCover::default();
        let installed_key = |installed: &MilkdropCover| {
            installed
                .installed
                .as_ref()
                .map(|(_, stamp)| stamp.reset_key)
        };
        sources.model.player = playing("/music/a.flac");
        cover.refresh(&sources.scene(), area);
        let first = installed_key(&cover);
        cover.refresh(&sources.scene(), area);
        assert_eq!(installed_key(&cover), first);
        let a = PathBuf::from("/music/a.flac");
        assert_eq!(first, Some((milkdrop_seed(Some(&a)), 12, 6)));
        sources.model.player = playing("/music/b.flac");
        cover.refresh(&sources.scene(), area);
        let b = PathBuf::from("/music/b.flac");
        assert_eq!(
            installed_key(&cover),
            Some((milkdrop_seed(Some(&b)), 12, 6))
        );
    }

    #[test]
    fn catch_up_steps_advance_on_their_own_step_times() {
        let mut sources = SceneSources::new(model_with_tracks(1));
        sources.model.player = playing("/music/b.flac");
        sources.spectrum.fill(0.6);
        let area = Some(Rect::new(0, 0, 24, 12));
        let mut stepwise_cover = MilkdropCover::default();
        let mut caught_up_cover = MilkdropCover::default();
        let mut scene = sources.scene();
        stepwise_cover.refresh(&scene, area);
        caught_up_cover.refresh(&scene, area);
        scene.presentation.since_first_paint = Duration::from_millis(35);
        stepwise_cover.refresh(&scene, area);
        scene.presentation.since_first_paint = Duration::from_millis(70);
        let stepwise_lines = text(stepwise_cover.refresh(&scene, area));
        let caught_up_lines = text(caught_up_cover.refresh(&scene, area));
        assert_eq!(stepwise_lines, caught_up_lines);
        assert_eq!(stepwise_cover.installed, caught_up_cover.installed);
    }

    #[rstest]
    #[case::whole_steps(330)]
    #[case::with_a_part_step_left_over(340)]
    fn a_gap_longer_than_four_steps_advances_four_and_drops_the_rest(
        #[case] gap_millis: u64,
    ) {
        let mut sources = SceneSources::new(model_with_tracks(1));
        sources.model.player = playing("/music/b.flac");
        sources.spectrum.fill(0.6);
        let area = Some(Rect::new(0, 0, 24, 12));
        let mut stalled_cover = MilkdropCover::default();
        let mut stepwise_cover = MilkdropCover::default();
        let mut scene = sources.scene();
        stalled_cover.refresh(&scene, area);
        for millis in [0, 33, 66, 99, 132] {
            scene.presentation.since_first_paint = Duration::from_millis(millis);
            stepwise_cover.refresh(&scene, area);
        }
        scene.presentation.since_first_paint = Duration::from_millis(gap_millis);
        let stalled_lines = text(stalled_cover.refresh(&scene, area));
        assert_eq!(stalled_lines, stepwise_cover.lines);
        assert_eq!(
            stalled_cover
                .installed
                .as_ref()
                .map(|(_, stamp)| stamp.since_first_paint),
            Some(Duration::from_millis(gap_millis))
        );
    }

    #[test]
    fn a_theme_change_while_paused_keeps_the_field_and_recolours_it() {
        let mut sources = SceneSources::new(model_with_tracks(1));
        sources.model.player = playing("/music/a.flac");
        sources.spectrum.fill(0.6);
        let area = Some(Rect::new(0, 0, 24, 12));
        let mut cover = MilkdropCover::default();
        {
            let mut scene = sources.scene();
            for millis in [0, 33, 66, 99] {
                scene.presentation.since_first_paint = Duration::from_millis(millis);
                cover.refresh(&scene, area);
            }
        }
        sources.model.player = Player::Paused {
            track: Arc::new(Track::listed(Path::new("/music/a.flac"))),
            position: Duration::ZERO,
            by: PausedBy::Listener,
        };
        let before = {
            let mut scene = sources.scene();
            scene.presentation.since_first_paint = Duration::from_millis(99);
            text(cover.refresh(&scene, area))
        };
        switch_to_ember(&mut sources);
        let after = {
            let mut scene = sources.scene();
            scene.presentation.since_first_paint = Duration::from_millis(132);
            text(cover.refresh(&scene, area))
        };
        let glyphs = |lines: &[Line<'static>]| {
            lines
                .iter()
                .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
                .collect::<Vec<_>>()
        };
        let colours = |lines: &[Line<'static>]| {
            lines
                .iter()
                .flat_map(|line| line.spans.iter().map(|span| span.style.fg))
                .collect::<Vec<_>>()
        };
        assert_eq!(glyphs(&before), glyphs(&after));
        assert!(glyphs(&after).iter().any(|glyph| glyph != " "));
        assert_ne!(colours(&before), colours(&after));
    }
}
