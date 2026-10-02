use std::{
    hash::{Hash, Hasher},
    path::Path,
    sync::Arc,
    time::Duration,
};

use ratatui::{layout::Rect, text::Line};

use crate::{
    CoverArt,
    MilkdropAdvance,
    MilkdropColors,
    MilkdropField,
    Playing,
    Scene,
    lines_into,
};

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
pub struct MilkdropCover {
    installed: Option<(MilkdropField, MilkdropTick)>,
    rows: Vec<Line<'static>>,
    lines: Arc<[Line<'static>]>,
}

impl MilkdropCover {
    pub fn refresh(&mut self, scene: &Scene<'_>, cover: Option<Rect>) -> CoverArt {
        let Some(rect) = cover else {
            self.installed = None;
            self.lines = Arc::default();
            return CoverArt::Missing;
        };
        let width = usize::from(rect.width);
        let height = usize::from(rect.height);
        let track = scene.player.current().map(|track| track.path());
        let seed = milkdrop_seed(track);
        let playing = if scene.player.is_playing() {
            Playing::Yes
        } else {
            Playing::No
        };
        let desired = MilkdropTick {
            reset_key: (seed, width, height),
            clock: scene.clock,
            playing,
        };
        let plan =
            plan_milkdrop(self.installed.as_ref().map(|(_, tick)| *tick), desired);
        if plan == MilkdropPlan::Rebuild {
            self.installed = Some((MilkdropField::new(width, height), desired));
        }
        if plan != MilkdropPlan::Reuse
            && let Some((field, tick)) = self.installed.as_mut()
        {
            field.advance(&MilkdropAdvance {
                bands: scene.spectrum,
                playing,
                seed,
                tick: u64::try_from(scene.clock.as_millis()).unwrap_or(u64::MAX),
            });
            *tick = desired;
            let colors = MilkdropColors::from_theme(&scene.active_theme());
            lines_into(field, &colors, &mut self.rows);
            self.lines = Arc::from(self.rows.as_slice());
        }
        CoverArt::Text(Arc::clone(&self.lines))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use rstest::rstest;

    use crate::{
        Playing,
        pixels::cover::milkdrop::{
            MilkdropPlan,
            MilkdropTick,
            milkdrop_seed,
            plan_milkdrop,
        },
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
