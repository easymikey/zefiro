use std::{
    hash::{Hash, Hasher},
    path::Path,
    time::Duration,
};

use ratatui::text::Line;
use widgets::{
    FrameLayout,
    MilkdropColors,
    MilkdropField,
    MilkdropStep,
    Playing,
    Scene,
    lines_into,
    step,
};

use crate::pixels::cover::CoverArtOwner;

#[derive(Debug, Clone, Copy)]
pub(crate) struct MilkdropSources<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: FrameLayout,
}

type MilkdropResetKey = (u64, usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MilkdropTick {
    reset_key: MilkdropResetKey,
    clock: Duration,
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
    } else if installed.clock != desired.clock {
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
    lines: Vec<Line<'static>>,
}

impl MilkdropCover {
    pub(crate) fn refresh(&mut self, sources: MilkdropSources<'_>) -> CoverArtOwner {
        let MilkdropSources { scene, layout } = sources;
        let Some(rect) = layout.cover else {
            self.field = None;
            self.tick = None;
            self.lines.clear();
            return CoverArtOwner::Missing;
        };
        let width = usize::from(rect.width);
        let height = usize::from(rect.height);
        let seed =
            milkdrop_seed(scene.model.player.current().map(|track| track.path()));
        let desired = MilkdropTick {
            reset_key: (seed, width, height),
            clock: scene.clock,
        };
        match plan_milkdrop(self.tick, desired) {
            MilkdropPlan::Rebuild => {
                self.field = Some(MilkdropField::new(width, height));
                self.advance(&scene, seed);
            }
            MilkdropPlan::Advance => self.advance(&scene, seed),
            MilkdropPlan::Reuse => {}
        }
        self.tick = Some(desired);
        self.paint(&scene);
        CoverArtOwner::Text(self.lines.clone())
    }

    fn advance(&mut self, scene: &Scene<'_>, seed: u64) {
        let Some(field) = self.field.as_mut() else {
            return;
        };
        let playing = if scene.model.player.is_playing() {
            Playing::Yes
        } else {
            Playing::No
        };
        let tick = u64::try_from(scene.clock.as_millis()).unwrap_or(u64::MAX);
        let input = MilkdropStep {
            bands: scene.spectrum,
            playing,
            seed,
            tick,
        };
        step(field, &input);
    }

    fn paint(&mut self, scene: &Scene<'_>) {
        let Some(field) = self.field.as_ref() else {
            self.lines.clear();
            return;
        };
        let colors = MilkdropColors::from_theme(&scene.active_theme());
        lines_into(field, &colors, &mut self.lines);
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use rstest::rstest;

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
        }
    }

    #[rstest]
    #[case::same_reset_key_and_clock(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 100), MilkdropPlan::Reuse)]
    #[case::a_new_clock_advances(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 116), MilkdropPlan::Advance)]
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
