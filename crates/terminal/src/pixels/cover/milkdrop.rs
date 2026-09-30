use std::{
    hash::{Hash, Hasher},
    path::Path,
    sync::Arc,
    time::Duration,
};

use ratatui::text::Line;
use widgets::{
    FrameLayout,
    MilkdropAdvance,
    MilkdropColors,
    MilkdropField,
    Playing,
    lines_into,
};

use crate::pixels::cover::{CoverMoment, OwnedCoverArt};

#[derive(Debug, Clone, Copy)]
pub(crate) struct MilkdropParts<'a> {
    pub(crate) moment: CoverMoment<'a>,
    pub(crate) colors: MilkdropColors,
    pub(crate) layout: FrameLayout,
}

type MilkdropResetKey = (u64, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MilkdropTick {
    reset_key: MilkdropResetKey,
    clock: Duration,
    playing: Playing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MilkdropPlan {
    Rebuild,
    Advance,
    Reuse,
}

#[must_use]
fn plan_milkdrop(
    installed: Option<MilkdropTick>,
    desired: MilkdropTick,
) -> MilkdropPlan {
    let Some(installed) = installed else {
        return MilkdropPlan::Rebuild;
    };
    if installed.reset_key != desired.reset_key {
        MilkdropPlan::Rebuild
    } else if installed.clock != desired.clock && desired.playing == Playing::Yes {
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
pub(crate) struct MilkdropCover {
    field: Option<MilkdropField>,
    tick: Option<MilkdropTick>,
    rows: Vec<Line<'static>>,
    lines: Arc<[Line<'static>]>,
}

impl MilkdropCover {
    pub(crate) fn refresh(&mut self, sources: MilkdropParts<'_>) -> OwnedCoverArt {
        let MilkdropParts {
            moment,
            colors,
            layout,
        } = sources;
        let Some(rect) = layout.cover else {
            self.field = None;
            self.tick = None;
            self.lines = Arc::default();
            return OwnedCoverArt::Missing;
        };
        let width = usize::from(rect.width);
        let height = usize::from(rect.height);
        let seed = milkdrop_seed(moment.track);
        let desired = MilkdropTick {
            reset_key: (seed, width, height),
            clock: moment.clock,
            playing: moment.playing,
        };
        match plan_milkdrop(self.tick, desired) {
            MilkdropPlan::Rebuild => {
                self.field = Some(MilkdropField::new(width, height));
                self.advance(moment, seed);
                self.paint(colors);
                self.tick = Some(desired);
            }
            MilkdropPlan::Advance => {
                self.advance(moment, seed);
                self.paint(colors);
                self.tick = Some(desired);
            }
            MilkdropPlan::Reuse => {}
        }
        OwnedCoverArt::Text(Arc::clone(&self.lines))
    }

    fn advance(&mut self, moment: CoverMoment<'_>, seed: u64) {
        let Some(field) = self.field.as_mut() else {
            return;
        };
        let tick = u64::try_from(moment.clock.as_millis()).unwrap_or(u64::MAX);
        let input = MilkdropAdvance {
            bands: moment.bands,
            playing: moment.playing,
            seed,
            tick,
        };
        field.advance(&input);
    }

    fn paint(&mut self, colors: MilkdropColors) {
        let Some(field) = self.field.as_ref() else {
            self.lines = Arc::default();
            return;
        };
        lines_into(field, &colors, &mut self.rows);
        self.lines = Arc::from(self.rows.as_slice());
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use rstest::rstest;
    use widgets::Playing;

    use crate::pixels::cover::milkdrop::{
        MilkdropPlan,
        MilkdropTick,
        milkdrop_seed,
        plan_milkdrop,
    };

    fn tick(reset_key: (u64, usize, usize), millis: u64) -> MilkdropTick {
        MilkdropTick {
            reset_key,
            clock: Duration::from_millis(millis),
            playing: Playing::Yes,
        }
    }

    fn paused(reset_key: (u64, usize, usize), millis: u64) -> MilkdropTick {
        MilkdropTick {
            playing: Playing::No,
            ..tick(reset_key, millis)
        }
    }

    #[rstest]
    #[case::same_reset_key_and_clock(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 100), MilkdropPlan::Reuse)]
    #[case::a_new_clock_advances(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 116), MilkdropPlan::Advance)]
    #[case::paused_clock_movement_reuses(Some(tick((1, 20, 8), 100)), paused((1, 20, 8), 116), MilkdropPlan::Reuse)]
    #[case::paused_reset_key_change_rebuilds(Some(tick((2, 20, 8), 100)), paused((1, 20, 8), 116), MilkdropPlan::Rebuild)]
    #[case::a_different_reset_key_rebuilds(Some(tick((2, 20, 8), 100)), tick((1, 20, 8), 100), MilkdropPlan::Rebuild)]
    #[case::nothing_installed_rebuilds(None, tick((1, 20, 8), 100), MilkdropPlan::Rebuild)]
    fn plan_milkdrop_decides_rebuild_advance_or_reuse(
        #[case] installed: Option<MilkdropTick>,
        #[case] desired: MilkdropTick,
        #[case] expected: MilkdropPlan,
    ) {
        assert_eq!(plan_milkdrop(installed, desired), expected);
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
}
