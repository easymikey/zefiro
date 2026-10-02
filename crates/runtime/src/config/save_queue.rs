use std::time::{Duration, Instant};

use config::AppearancePatch;
use kernel::ConfigPatch;

#[derive(Debug, PartialEq)]
pub(crate) enum Saves<C, A> {
    Config(C),
    Appearance(A),
    Both { config: C, appearance: A },
}

pub(crate) type SavePatches = Saves<ConfigPatch, AppearancePatch>;

impl<C, A> Saves<C, A> {
    fn from_options(config: Option<C>, appearance: Option<A>) -> Option<Self> {
        match (config, appearance) {
            (Some(config), Some(appearance)) => Some(Self::Both { config, appearance }),
            (Some(config), None) => Some(Self::Config(config)),
            (None, Some(appearance)) => Some(Self::Appearance(appearance)),
            (None, None) => None,
        }
    }

    pub(crate) fn map<C2, A2>(
        self,
        config: impl FnOnce(C) -> C2,
        appearance: impl FnOnce(A) -> A2,
    ) -> Saves<C2, A2> {
        match self {
            Self::Config(c) => Saves::Config(config(c)),
            Self::Appearance(a) => Saves::Appearance(appearance(a)),
            Self::Both {
                config: c,
                appearance: a,
            } => {
                let config = config(c);
                Saves::Both {
                    config,
                    appearance: appearance(a),
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
struct PendingSave<P> {
    patch: P,
    deadline: Instant,
}

#[derive(Debug, PartialEq)]
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

    pub(crate) fn queue_config(&mut self, now: Instant, patch: ConfigPatch) {
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
            .map_or(patch, |prev| prev.patch.then(patch));
        self.pending_appearance = Some(PendingSave {
            patch: merged,
            deadline: now + self.debounce,
        });
    }

    pub(crate) fn take_due(&mut self, now: Instant) -> Option<SavePatches> {
        let config = take_if_due(&mut self.pending_config, now);
        let appearance = take_if_due(&mut self.pending_appearance, now);
        Saves::from_options(config, appearance)
    }

    pub(crate) fn take_all(&mut self) -> Option<SavePatches> {
        let config = self.pending_config.take().map(|pending| pending.patch);
        let appearance = self.pending_appearance.take().map(|pending| pending.patch);
        Saves::from_options(config, appearance)
    }
}

fn take_if_due<P>(slot: &mut Option<PendingSave<P>>, now: Instant) -> Option<P> {
    if slot.as_ref().is_some_and(|pending| pending.deadline <= now) {
        slot.take().map(|pending| pending.patch)
    } else {
        None
    }
}

fn merge_config_patch(base: Option<ConfigPatch>, next: ConfigPatch) -> ConfigPatch {
    let Some(base) = base else {
        return next;
    };
    ConfigPatch {
        crossfade: next.crossfade.or(base.crossfade),
        device: next.device.or(base.device),
        replay_gain: next.replay_gain.or(base.replay_gain),
        theme: next.theme.or(base.theme),
        volume: next.volume.or(base.volume),
        sleep_presets: next.sleep_presets.or(base.sleep_presets),
        music_dir: next.music_dir.or(base.music_dir),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        Bounded,
        ConfigPatch,
        domain::{Crossfade, DeviceName, OutputDevice, ThemeName},
    };

    use crate::config::save_queue::merge_config_patch;

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
            .device(OutputDevice::Named(
                DeviceName::new("Speakers".to_string()).unwrap(),
            ))
            .build();
        let later = ConfigPatch::builder().build();

        let merged = merge_config_patch(Some(earlier), later);

        assert_eq!(
            merged.device,
            Some(OutputDevice::Named(
                DeviceName::new("Speakers".to_string()).unwrap()
            ))
        );
    }
}
