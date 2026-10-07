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

type MilkdropResetKey = (u64, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MilkdropStamp {
    reset_key: MilkdropResetKey,
    theme_revision: Revision,
    since_first_paint: Duration,
    playback: Playback,
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
        && desired_stamp.playback == Playback::Playing
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
        let seed =
            self.track_seed_for(scene.player.current().map(|track| track.path()));
        let desired_stamp = MilkdropStamp {
            reset_key: (seed, width, height),
            theme_revision: scene.revisions.theme,
            since_first_paint: scene.presentation.since_first_paint,
            playback: Playback::from(scene.player),
        };
        let installed = self.installed.as_ref().map(|(_, stamp)| *stamp);
        let plan = plan_milkdrop(installed, desired_stamp);
        let since_first_paint = desired_stamp.since_first_paint;
        let start =
            installed.map_or(since_first_paint, |stamp| stamp.since_first_paint);
        let whole = elapsed_steps(installed, since_first_paint);
        let (steps, first_step, advanced_to) = match plan {
            MilkdropPlan::Rebuild => (1, since_first_paint, since_first_paint),
            MilkdropPlan::Recolour | MilkdropPlan::Reuse => (0, start, start),
            MilkdropPlan::Advance if whole <= 4 => {
                (whole, start + STEP, start + STEP * whole)
            }
            MilkdropPlan::Advance => (4, start + STEP, since_first_paint),
        };
        if plan == MilkdropPlan::Rebuild {
            self.installed = Some((MilkdropField::new(width, height), desired_stamp));
        }
        if plan != MilkdropPlan::Reuse
            && let Some((field, installed_stamp)) = self.installed.as_mut()
        {
            for step in (0..steps).map(|step| first_step + STEP * step) {
                field.advance(&MilkdropAdvance {
                    spectrum: scene.presentation.spectrum,
                    playback: desired_stamp.playback,
                    seed,
                    tick: u64::try_from(step.as_millis()).unwrap_or(u64::MAX),
                });
            }
            *installed_stamp = MilkdropStamp {
                since_first_paint: advanced_to,
                ..desired_stamp
            };
            let style = MilkdropStyle::from_theme(&scene.active_theme());
            self.lines = lines(field, &style);
        }
        CardCover::Text(Arc::clone(&self.lines))
    }
}

fn elapsed_steps(
    milkdrop_stamp: Option<MilkdropStamp>,
    since_first_paint: Duration,
) -> u32 {
    milkdrop_stamp.map_or(0, |stamp| {
        let elapsed = since_first_paint.saturating_sub(stamp.since_first_paint);
        u32::try_from(elapsed.as_millis() / STEP.as_millis()).unwrap_or(u32::MAX)
    })
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
            playback: Playback::Playing,
        }
    }

    fn paused(reset_key: (u64, usize, usize), millis: u64) -> MilkdropStamp {
        MilkdropStamp {
            playback: Playback::Paused,
            ..stamp(reset_key, millis)
        }
    }

    fn themed(milkdrop_stamp: MilkdropStamp) -> MilkdropStamp {
        MilkdropStamp {
            theme_revision: Revision::default().next(),
            ..milkdrop_stamp
        }
    }

    #[rstest]
    #[case::same_reset_key_and_clock(Some(stamp((1, 20, 8), 100)), stamp((1, 20, 8), 100), MilkdropPlan::Reuse)]
    #[case::a_new_clock_advances(Some(stamp((1, 20, 8), 100)), stamp((1, 20, 8), 133), MilkdropPlan::Advance)]
    #[case::less_than_a_step_reuses(Some(stamp((1, 20, 8), 100)), stamp((1, 20, 8), 132), MilkdropPlan::Reuse)]
    #[case::a_clock_gone_backwards_rebuilds(Some(stamp((1, 20, 8), 100)), stamp((1, 20, 8), 50), MilkdropPlan::Rebuild)]
    #[case::paused_clock_movement_reuses(Some(stamp((1, 20, 8), 100)), paused((1, 20, 8), 133), MilkdropPlan::Reuse)]
    #[case::paused_reset_key_change_rebuilds(Some(stamp((2, 20, 8), 100)), paused((1, 20, 8), 133), MilkdropPlan::Rebuild)]
    #[case::a_different_reset_key_rebuilds(Some(stamp((2, 20, 8), 100)), stamp((1, 20, 8), 100), MilkdropPlan::Rebuild)]
    #[case::a_new_theme_rebuilds_even_when_paused(Some(stamp((1, 20, 8), 100)), themed(paused((1, 20, 8), 100)), MilkdropPlan::Recolour)]
    #[case::nothing_installed_rebuilds(None, stamp((1, 20, 8), 100), MilkdropPlan::Rebuild)]
    fn plan_milkdrop_decides_rebuild_advance_or_reuse(
        #[case] installed_stamp: Option<MilkdropStamp>,
        #[case] desired_stamp: MilkdropStamp,
        #[case] expected: MilkdropPlan,
    ) {
        assert_eq!(plan_milkdrop(installed_stamp, desired_stamp), expected);
    }

    #[test]
    fn the_same_path_always_seeds_the_same_way() {
        let path = PathBuf::from("/music/a.flac");
        assert_eq!(milkdrop_seed(Some(&path)), milkdrop_seed(Some(&path)));
    }

    #[test]
    fn different_paths_seed_differently() {
        let a = PathBuf::from("/music/a.flac");
        let b = PathBuf::from("/music/b.flac");
        assert_ne!(milkdrop_seed(Some(&a)), milkdrop_seed(Some(&b)));
    }

    #[test]
    fn no_track_still_seeds_deterministically() {
        assert_eq!(milkdrop_seed(None), milkdrop_seed(None));
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

    #[test]
    fn a_theme_change_while_paused_recolours_the_lines() {
        let mut sources = SceneSources::new(model_with_tracks(1));
        let area = Some(Rect::new(0, 0, 12, 6));
        let mut cover = MilkdropCover::default();
        let before = cover.refresh(&sources.scene(), area);
        switch_to_ember(&mut sources);
        let after = cover.refresh(&sources.scene(), area);
        let (CardCover::Text(before), CardCover::Text(after)) = (before, after) else {
            panic!("milkdrop paints text lines");
        };
        assert_ne!(before, after);
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
    fn paints_between_steps_advance_as_much_as_one_paint_later() {
        let mut sources = SceneSources::new(model_with_tracks(1));
        sources.model.player = Player::Playing {
            track: Arc::new(Track::listed(Path::new("/music/a.flac"))),
            playhead: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        };
        let area = Some(Rect::new(0, 0, 12, 6));
        let mut once_cover = MilkdropCover::default();
        let mut twice_cover = MilkdropCover::default();
        let mut scene = sources.scene();
        let start = text(once_cover.refresh(&scene, area));
        twice_cover.refresh(&scene, area);
        scene.presentation.since_first_paint = Duration::from_millis(10);
        twice_cover.refresh(&scene, area);
        scene.presentation.since_first_paint = Duration::from_millis(20);
        let once_lines = text(once_cover.refresh(&scene, area));
        let twice_lines = text(twice_cover.refresh(&scene, area));
        assert_eq!(once_lines, twice_lines);
        assert_eq!(once_lines, start);
        scene.presentation.since_first_paint = Duration::from_millis(40);
        assert_ne!(text(once_cover.refresh(&scene, area)), start);
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
