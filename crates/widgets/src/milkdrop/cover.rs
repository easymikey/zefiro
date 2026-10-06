use std::{
    hash::{Hash, Hasher},
    path::Path,
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
struct MilkdropTick {
    reset_key: MilkdropResetKey,
    theme: Revision,
    clock: Duration,
    playing: Playback,
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
    if installed.reset_key != desired.reset_key
        || installed.theme != desired.theme
        || desired.clock < installed.clock
    {
        MilkdropPlan::Rebuild
    } else if desired.clock - installed.clock >= STEP
        && desired.playing == Playback::Playing
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
    installed: Option<(MilkdropField, MilkdropTick)>,
    lines: Arc<[Line<'static>]>,
}

impl MilkdropCover {
    pub fn refresh(&mut self, scene: &Scene<'_>, cover: Option<Rect>) -> CardCover {
        let Some(rect) = cover else {
            self.installed = None;
            self.lines = Arc::default();
            return CardCover::Missing;
        };
        let width = usize::from(rect.width);
        let height = usize::from(rect.height);
        let track = scene.player.current().map(|track| track.path());
        let seed = milkdrop_seed(track);
        let playing = if scene.player.is_playing() {
            Playback::Playing
        } else {
            Playback::Paused
        };
        let desired = MilkdropTick {
            reset_key: (seed, width, height),
            theme: scene.revisions.theme,
            clock: scene.presentation.clock,
            playing,
        };
        let installed = self.installed.as_ref().map(|(_, tick)| *tick);
        let plan = plan_milkdrop(installed, desired);
        let whole = installed.map_or(0, |tick| {
            let elapsed = desired.clock.saturating_sub(tick.clock);
            u32::try_from(elapsed.as_millis() / STEP.as_millis()).unwrap_or(u32::MAX)
        });
        let steps = match plan {
            MilkdropPlan::Rebuild => 1,
            MilkdropPlan::Advance => whole.min(4),
            MilkdropPlan::Reuse => 0,
        };
        let stamp = match installed {
            Some(tick) if plan == MilkdropPlan::Advance && whole <= 4 => {
                tick.clock + STEP * steps
            }
            Some(_) | None => desired.clock,
        };
        if plan == MilkdropPlan::Rebuild {
            self.installed = Some((MilkdropField::new(width, height), desired));
        }
        if plan != MilkdropPlan::Reuse
            && let Some((field, tick)) = self.installed.as_mut()
        {
            for _ in 0..steps {
                field.advance(&MilkdropAdvance {
                    bands: scene.presentation.spectrum,
                    playing,
                    seed,
                    tick: u64::try_from(scene.presentation.clock.as_millis())
                        .unwrap_or(u64::MAX),
                });
            }
            *tick = MilkdropTick {
                clock: stamp,
                ..desired
            };
            let style = MilkdropStyle::from_theme(&scene.active_theme());
            self.lines = lines(field, &style);
        }
        CardCover::Text(Arc::clone(&self.lines))
    }
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
            player::Player,
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
            MilkdropTick,
            milkdrop_seed,
            plan_milkdrop,
        },
        test_support::{SceneSources, model_with_tracks, stock_theme},
        theme::colors::ThemeBase,
    };

    fn tick(reset_key: (u64, usize, usize), millis: u64) -> MilkdropTick {
        MilkdropTick {
            reset_key,
            theme: Revision::default(),
            clock: Duration::from_millis(millis),
            playing: Playback::Playing,
        }
    }

    fn paused(reset_key: (u64, usize, usize), millis: u64) -> MilkdropTick {
        MilkdropTick {
            playing: Playback::Paused,
            ..tick(reset_key, millis)
        }
    }

    fn themed(tick: MilkdropTick) -> MilkdropTick {
        MilkdropTick {
            theme: Revision::default().next(),
            ..tick
        }
    }

    #[rstest]
    #[case::same_reset_key_and_clock(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 100), MilkdropPlan::Reuse)]
    #[case::a_new_clock_advances(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 133), MilkdropPlan::Advance)]
    #[case::less_than_a_step_reuses(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 132), MilkdropPlan::Reuse)]
    #[case::a_clock_gone_backwards_rebuilds(Some(tick((1, 20, 8), 100)), tick((1, 20, 8), 50), MilkdropPlan::Rebuild)]
    #[case::paused_clock_movement_reuses(Some(tick((1, 20, 8), 100)), paused((1, 20, 8), 133), MilkdropPlan::Reuse)]
    #[case::paused_reset_key_change_rebuilds(Some(tick((2, 20, 8), 100)), paused((1, 20, 8), 133), MilkdropPlan::Rebuild)]
    #[case::a_different_reset_key_rebuilds(Some(tick((2, 20, 8), 100)), tick((1, 20, 8), 100), MilkdropPlan::Rebuild)]
    #[case::a_new_theme_rebuilds_even_when_paused(Some(tick((1, 20, 8), 100)), themed(paused((1, 20, 8), 100)), MilkdropPlan::Rebuild)]
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

    #[test]
    fn a_theme_change_while_paused_recolours_the_lines() {
        let mut sources = SceneSources::new(model_with_tracks(1));
        let area = Some(Rect::new(0, 0, 12, 6));
        let mut cover = MilkdropCover::default();
        let before = cover.refresh(&sources.scene(), area);
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
        scene.presentation.clock = Duration::from_millis(10);
        twice_cover.refresh(&scene, area);
        scene.presentation.clock = Duration::from_millis(20);
        let once_lines = text(once_cover.refresh(&scene, area));
        let twice_lines = text(twice_cover.refresh(&scene, area));
        assert_eq!(once_lines, twice_lines);
        assert_eq!(once_lines, start);
        scene.presentation.clock = Duration::from_millis(40);
        assert_ne!(text(once_cover.refresh(&scene, area)), start);
    }
}
