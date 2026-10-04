use std::time::Duration;

use kernel::{
    Cmd,
    ConfigEvent,
    ConfigPatch,
    domain::{Revision, appearance::AppearancePatch},
    update::Unhandled,
};

use crate::driver::ConfigEffect;

pub(crate) const SAVE_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Default, PartialEq)]
pub(crate) struct SaveQueue {
    config: Option<ConfigPatch>,
    appearance: Option<AppearancePatch>,
    revision: Revision,
}

impl SaveQueue {
    pub(crate) fn revision(&self) -> Revision {
        self.revision
    }

    pub(crate) fn queue_config(&mut self, patch: ConfigPatch) {
        let merged = match self.config.take() {
            Some(base) => merge_config_patch(base, patch),
            None => patch,
        };
        self.config = Some(merged);
        self.revision.advance();
    }

    pub(crate) fn queue_appearance(&mut self, patch: AppearancePatch) {
        let merged = self.appearance.map_or(patch, |earlier| earlier.then(patch));
        self.appearance = Some(merged);
        self.revision.advance();
    }

    pub(crate) fn wait_since(
        &self,
        before: Revision,
    ) -> Cmd<ConfigEffect, ConfigEvent> {
        if self.revision == before || self.is_empty() {
            return Cmd::none();
        }
        Cmd::effect(ConfigEffect::After {
            delay: SAVE_DEBOUNCE,
            revision: self.revision,
        })
    }

    pub(crate) fn flush(&mut self) -> Cmd<ConfigEffect, ConfigEvent> {
        if self.is_empty() {
            return Cmd::none();
        }
        self.revision.advance();
        self.drain()
    }

    fn is_empty(&self) -> bool {
        self.config.is_none() && self.appearance.is_none()
    }

    fn drain(&mut self) -> Cmd<ConfigEffect, ConfigEvent> {
        let config = self.config.take().map(ConfigEffect::SaveConfig);
        let appearance = self.appearance.take().map(ConfigEffect::SaveAppearance);
        config.into_iter().chain(appearance).collect()
    }

    pub(crate) fn elapsed(
        &mut self,
        revision: Revision,
    ) -> Result<Cmd<ConfigEffect, ConfigEvent>, Unhandled> {
        if revision != self.revision {
            return Err(Unhandled);
        }
        if self.is_empty() {
            return Err(Unhandled);
        }
        Ok(self.drain())
    }
}

fn merge_config_patch(base: ConfigPatch, next: ConfigPatch) -> ConfigPatch {
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

    use crate::driver::saves::merge_config_patch;

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

        let merged = merge_config_patch(earlier, later);

        assert_eq!(merged.theme.as_ref().map(ThemeName::as_str), Some("dark"));
        assert_eq!(merged.crossfade, Some(crossfade(3)));
    }

    #[test]
    fn merge_config_patch_device_keep_does_not_override() {
        let speakers =
            || OutputDevice::Named(DeviceName::new("Speakers".to_string()).unwrap());
        let earlier = ConfigPatch::builder().device(speakers()).build();

        let merged = merge_config_patch(earlier, ConfigPatch::builder().build());

        assert_eq!(merged.device, Some(speakers()));
    }
}
