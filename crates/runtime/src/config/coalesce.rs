use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use config::AppearancePatch;
use kernel::{ConfigPatch, DevicePatch};

use crate::{
    config::{
        ConfigPaths,
        write::{Written, save, save_appearance},
    },
    error::SaveError,
};

#[derive(Debug, Default)]
pub(crate) struct Flushed {
    pub config: Option<Result<Written, SaveError>>,
    pub appearance: Option<Result<Written, SaveError>>,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SavePatches {
    pub config: Option<ConfigPatch>,
    pub appearance: Option<AppearancePatch>,
}

#[derive(Debug, PartialEq)]
struct PendingSave<P> {
    patch: P,
    deadline: Instant,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct SaveQueue {
    debounce: Duration,
    pending_config: Option<PendingSave<ConfigPatch>>,
    pending_appearance: Option<PendingSave<AppearancePatch>>,
}

impl SaveQueue {
    #[must_use]
    pub(crate) fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            pending_config: None,
            pending_appearance: None,
        }
    }

    #[must_use]
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        [
            self.pending_config.as_ref().map(|pending| pending.deadline),
            self.pending_appearance
                .as_ref()
                .map(|pending| pending.deadline),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(crate) fn queue(&mut self, now: Instant, patch: ConfigPatch) {
        let base = self.pending_config.take().map(|pending| pending.patch);
        self.pending_config = Some(PendingSave {
            patch: merge_config_patch(base, patch),
            deadline: now + self.debounce,
        });
    }

    pub(crate) fn queue_appearance(&mut self, now: Instant, patch: AppearancePatch) {
        let merged = self
            .pending_appearance
            .take()
            .map_or(patch, |prev| merge_appearance_patch(prev.patch, patch));
        self.pending_appearance = Some(PendingSave {
            patch: merged,
            deadline: now + self.debounce,
        });
    }

    pub(crate) fn take_due(&mut self, now: Instant) -> Option<SavePatches> {
        let config = take_if_due(&mut self.pending_config, now);
        let appearance = take_if_due(&mut self.pending_appearance, now);
        gather(config, appearance)
    }

    pub(crate) fn take_all(&mut self) -> Option<SavePatches> {
        let config = self.pending_config.take().map(|pending| pending.patch);
        let appearance = self.pending_appearance.take().map(|pending| pending.patch);
        gather(config, appearance)
    }
}

fn take_if_due<P>(slot: &mut Option<PendingSave<P>>, now: Instant) -> Option<P> {
    if slot.as_ref().is_some_and(|pending| pending.deadline <= now) {
        slot.take().map(|pending| pending.patch)
    } else {
        None
    }
}

fn gather(
    config: Option<ConfigPatch>,
    appearance: Option<AppearancePatch>,
) -> Option<SavePatches> {
    if config.is_none() && appearance.is_none() {
        None
    } else {
        Some(SavePatches { config, appearance })
    }
}

fn merge_config_patch(base: Option<ConfigPatch>, next: ConfigPatch) -> ConfigPatch {
    let Some(base) = base else {
        return next;
    };
    ConfigPatch {
        crossfade: next.crossfade.or(base.crossfade),
        device: match next.device {
            DevicePatch::Keep => base.device,
            overriding @ (DevicePatch::SystemDefault | DevicePatch::Named(_)) => {
                overriding
            }
        },
        replaygain: next.replaygain.or(base.replaygain),
        theme: next.theme.or(base.theme),
        volume: next.volume.or(base.volume),
        sleep_presets: next.sleep_presets.or(base.sleep_presets),
        music_dir: next.music_dir.or(base.music_dir),
    }
}

fn merge_appearance_patch(
    base: AppearancePatch,
    next: AppearancePatch,
) -> AppearancePatch {
    AppearancePatch {
        cover_style: next.cover_style.or(base.cover_style),
        cover_brackets: next.cover_brackets.or(base.cover_brackets),
        format_chips: next.format_chips.or(base.format_chips),
        speed_chip: next.speed_chip.or(base.speed_chip),
        progress_remaining: next.progress_remaining.or(base.progress_remaining),
        key_hints: next.key_hints.or(base.key_hints),
        animations: next.animations.or(base.animations),
        layout_mode: next.layout_mode.or(base.layout_mode),
    }
}

#[derive(Debug)]
pub(crate) struct SavePaths {
    config: Option<PathBuf>,
    appearance: PathBuf,
}

impl SavePaths {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths) -> Self {
        Self {
            config: paths.config.clone(),
            appearance: paths.appearance.clone(),
        }
    }

    #[must_use]
    pub(crate) fn write(&self, patches: SavePatches) -> Flushed {
        Flushed {
            config: patches
                .config
                .map(|patch| run_save(self.config.as_ref(), patch)),
            appearance: patches
                .appearance
                .map(|patch| save_appearance(&self.appearance, patch)),
        }
    }
}

fn run_save(path: Option<&PathBuf>, patch: ConfigPatch) -> Result<Written, SaveError> {
    path.map_or(Err(SaveError::NoConfigDirectory), |path| save(path, patch))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use config::{CoverStyle, FormatChips, KeyHints};
    use kernel::{
        Bounded,
        ConfigPatch,
        DevicePatch,
        domain::{Crossfade, DeviceName, ThemeName},
    };

    use crate::{
        config::coalesce::{
            SavePatches,
            SavePaths,
            merge_appearance_patch,
            merge_config_patch,
        },
        error::SaveError,
    };

    fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(seconds))
    }

    fn theme_only(name: &'static str) -> SavePatches {
        SavePatches {
            config: Some(
                ConfigPatch::builder()
                    .theme(ThemeName::from_static(name))
                    .build(),
            ),
            appearance: None,
        }
    }

    #[test]
    fn merge_config_patch_folds_disjoint_fields_and_later_field_wins() {
        let earlier = ConfigPatch::builder()
            .theme(ThemeName::from_static("dark"))
            .crossfade(crossfade(1))
            .build();
        let later = ConfigPatch::builder().crossfade(crossfade(3)).build();

        let merged = merge_config_patch(Some(earlier), later);

        assert_eq!(merged.theme.as_ref().map(ThemeName::as_str), Some("dark"));
        assert_eq!(merged.crossfade, Some(crossfade(3)));
    }

    #[test]
    fn merge_config_patch_device_keep_does_not_override() {
        let earlier = ConfigPatch::builder()
            .device(DevicePatch::Named(
                DeviceName::new("Speakers".to_string()).unwrap(),
            ))
            .build();
        let later = ConfigPatch::builder().device(DevicePatch::Keep).build();

        let merged = merge_config_patch(Some(earlier), later);

        assert_eq!(
            merged.device,
            DevicePatch::Named(DeviceName::new("Speakers".to_string()).unwrap())
        );
    }

    #[test]
    fn merge_appearance_patch_folds_disjoint_fields_and_later_field_wins() {
        let earlier = config::AppearancePatch::builder()
            .format_chips(FormatChips::Hidden)
            .cover_style(CoverStyle::Vinyl)
            .build();
        let later = config::AppearancePatch::builder()
            .cover_style(CoverStyle::Off)
            .key_hints(KeyHints::Hidden)
            .build();

        let merged = merge_appearance_patch(earlier, later);

        assert_eq!(merged.format_chips, Some(FormatChips::Hidden));
        assert_eq!(merged.key_hints, Some(KeyHints::Hidden));
        assert_eq!(merged.cover_style, Some(CoverStyle::Off));
    }

    #[test]
    fn write_lands_a_config_patch_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let paths = SavePaths {
            config: Some(path.clone()),
            appearance: directory.path().join("window.toml"),
        };

        let flushed = paths.write(theme_only("dark"));

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("theme = \"dark\""));
        assert!(matches!(flushed.config, Some(Ok(_))));
        assert!(flushed.appearance.is_none());
    }

    #[test]
    fn a_save_with_no_path_reports_no_config_dir() {
        let directory = tempfile::tempdir().unwrap();
        let paths = SavePaths {
            config: None,
            appearance: directory.path().join("window.toml"),
        };

        let flushed = paths.write(theme_only("dark"));

        assert!(matches!(
            flushed.config,
            Some(Err(SaveError::NoConfigDirectory))
        ));
    }
}
