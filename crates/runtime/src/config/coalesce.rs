use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use config::AppearancePatch;
use kernel::{ConfigPatch, DevicePatch};

use crate::{
    config::write::{Written, save, save_appearance},
    error::SaveError,
};

#[derive(Debug, Default)]
pub(crate) struct Flushed {
    pub config: Option<Result<Written, SaveError>>,
    pub appearance: Option<Result<Written, SaveError>>,
}

#[derive(Debug)]
struct PendingSave<P> {
    patch: P,
    deadline: Instant,
}

#[must_use]
#[derive(Debug)]
pub(crate) struct SaveCoalescer {
    config: Option<PathBuf>,
    appearance: PathBuf,
    debounce: Duration,
    pending_config: Option<PendingSave<ConfigPatch>>,
    pending_appearance: Option<PendingSave<AppearancePatch>>,
}

impl SaveCoalescer {
    pub(crate) fn new(
        config: Option<PathBuf>,
        appearance: PathBuf,
        debounce: Duration,
    ) -> Self {
        Self {
            config,
            appearance,
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

    pub(crate) fn flush_due(&mut self, now: Instant) -> Flushed {
        let mut flushed = Flushed::default();
        if matches!(&self.pending_config, Some(pending) if pending.deadline <= now)
            && let Some(pending) = self.pending_config.take()
        {
            flushed.config = Some(run_save(&self.config, pending.patch));
        }
        if matches!(&self.pending_appearance, Some(pending) if pending.deadline <= now)
            && let Some(pending) = self.pending_appearance.take()
        {
            flushed.appearance = Some(save_appearance(&self.appearance, pending.patch));
        }
        flushed
    }

    pub(crate) fn flush_all(&mut self) -> Flushed {
        let mut flushed = Flushed::default();
        if let Some(pending) = self.pending_config.take() {
            flushed.config = Some(run_save(&self.config, pending.patch));
        }
        if let Some(pending) = self.pending_appearance.take() {
            flushed.appearance = Some(save_appearance(&self.appearance, pending.patch));
        }
        flushed
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

fn run_save(path: &Option<PathBuf>, patch: ConfigPatch) -> Result<Written, SaveError> {
    path.as_ref()
        .map_or(Err(SaveError::NoConfigDirectory), |path| save(path, patch))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use config::{CoverStyle, FormatChips, KeyHints};
    use kernel::{
        Bounded,
        ConfigPatch,
        DevicePatch,
        Percent,
        domain::{Crossfade, DeviceName, ThemeName},
    };

    use crate::{
        config::{
            ConfigTiming,
            coalesce::{SaveCoalescer, merge_appearance_patch, merge_config_patch},
        },
        error::SaveError,
    };

    fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(seconds))
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

    fn coalescer(directory: &tempfile::TempDir) -> SaveCoalescer {
        SaveCoalescer::new(
            Some(directory.path().join("config.toml")),
            directory.path().join("sifr-ui.toml"),
            ConfigTiming::default().save_debounce,
        )
    }

    #[test]
    fn a_burst_of_saves_becomes_one_write_carrying_every_field() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut coalescer = coalescer(&directory);
        let start = Instant::now();

        coalescer.queue(
            start,
            ConfigPatch::builder()
                .theme(ThemeName::from_static("noir"))
                .build(),
        );
        for volume in [10u8, 20, 30, 40, 50] {
            let now = start + Duration::from_millis(u64::from(volume));
            coalescer.queue(
                now,
                ConfigPatch::builder()
                    .volume(Percent::clamped(volume))
                    .build(),
            );
            assert_eq!(
                coalescer.next_deadline(),
                Some(now + ConfigTiming::default().save_debounce),
                "every new job in the burst pushes the trailing edge out"
            );
        }

        let flushed = coalescer.flush_due(start + Duration::from_secs(1));

        assert_eq!(
            coalescer.next_deadline(),
            None,
            "the flush leaves nothing pending"
        );
        assert!(matches!(flushed.config, Some(Ok(_))));
        assert!(
            flushed.appearance.is_none(),
            "a coalesced burst reports exactly one land"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("volume = 50"), "the last patch wins: {text}");
        assert!(
            text.contains("theme = \"noir\""),
            "and the first patch's disjoint field is still there: {text}"
        );
    }

    #[test]
    fn flush_all_writes_a_save_whose_window_has_not_elapsed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut coalescer = SaveCoalescer::new(
            Some(path.clone()),
            directory.path().join("window.toml"),
            ConfigTiming::default().save_debounce,
        );

        coalescer.queue(
            Instant::now(),
            ConfigPatch::builder()
                .theme(ThemeName::from_static("dark"))
                .build(),
        );
        let flushed = coalescer.flush_all();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("theme = \"dark\""));
        assert!(matches!(flushed.config, Some(Ok(_))));
    }

    #[test]
    fn a_save_with_no_path_reports_no_config_dir() {
        let directory = tempfile::tempdir().unwrap();
        let mut coalescer = SaveCoalescer::new(
            None,
            directory.path().join("window.toml"),
            ConfigTiming::default().save_debounce,
        );

        coalescer.queue(
            Instant::now(),
            ConfigPatch::builder()
                .theme(ThemeName::from_static("dark"))
                .build(),
        );
        let flushed = coalescer.flush_all();

        assert!(matches!(
            flushed.config,
            Some(Err(SaveError::NoConfigDirectory))
        ));
    }
}
