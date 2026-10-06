use std::time::Duration;

use kernel::{
    cmd::{Cmd, ConfigPatch},
    domain::{appearance::AppearancePatch, revision::Revision},
    update::machine::{LoopEffect, Unhandled},
};

use crate::driver::{
    effect::{ConfigEffect, ConfigLoopCmd},
    message::ConfigMessage,
};

pub(crate) const SAVE_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Default, PartialEq)]
pub(crate) struct PendingSaves {
    config_patch: Option<ConfigPatch>,
    appearance_patch: Option<AppearancePatch>,
    revision: Revision,
}

impl PendingSaves {
    pub(crate) fn revision(&self) -> Revision {
        self.revision
    }

    pub(crate) fn hold_config(&mut self, patch: ConfigPatch) {
        let merged = match self.config_patch.take() {
            Some(earlier) => earlier.then(patch),
            None => patch,
        };
        self.config_patch = Some(merged);
        self.revision.advance();
    }

    pub(crate) fn hold_appearance(&mut self, patch: AppearancePatch) {
        let merged = self
            .appearance_patch
            .map_or(patch, |earlier| earlier.then(patch));
        self.appearance_patch = Some(merged);
        self.revision.advance();
    }

    pub(crate) fn wait_since(&self, before: Revision) -> ConfigLoopCmd {
        if self.revision == before || self.is_empty() {
            return Cmd::none();
        }
        Cmd::effect(LoopEffect::After {
            delay: SAVE_DEBOUNCE,
            message: ConfigMessage::Elapsed(self.revision),
        })
    }

    pub(crate) fn flush(&mut self) -> ConfigLoopCmd {
        if self.is_empty() {
            return Cmd::none();
        }
        self.revision.advance();
        self.drain()
    }

    fn is_empty(&self) -> bool {
        self.config_patch.is_none() && self.appearance_patch.is_none()
    }

    fn drain(&mut self) -> ConfigLoopCmd {
        let config = self.config_patch.take().map(ConfigEffect::SaveConfig);
        let appearance = self
            .appearance_patch
            .take()
            .map(ConfigEffect::SaveAppearance);
        config
            .into_iter()
            .chain(appearance)
            .map(LoopEffect::Execute)
            .collect()
    }

    pub(crate) fn elapsed(
        &mut self,
        revision: Revision,
    ) -> Result<ConfigLoopCmd, Unhandled> {
        if revision != self.revision || self.is_empty() {
            return Err(Unhandled);
        }
        Ok(self.drain())
    }
}
