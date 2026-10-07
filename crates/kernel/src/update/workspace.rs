use std::collections::hash_map::Entry;

use crate::{
    cmd::{Cmd, Effect},
    domain::{
        config::{ConfigError, ConfigErrors, ConfigName},
        cue::Cue,
        keymap::KeymapOverrides,
        revision::{Revision, Revisions},
        toast::{TOAST_LIFETIME, TOAST_STACK, Toast},
        workspace::Workspace,
    },
    message::{ConfigReload, Timer},
    update::{
        keymap::bindings::Keymap,
        machine::{self, Unhandled},
    },
};

impl ConfigErrors {
    pub(crate) fn replace(
        &mut self,
        name: ConfigName,
        error: ConfigError,
    ) -> Result<(), Unhandled> {
        match self.0.entry(name) {
            Entry::Vacant(vacant) => {
                vacant.insert(error);
                Ok(())
            }
            Entry::Occupied(mut occupied) => {
                machine::replace(occupied.get_mut(), error)
            }
        }
    }
}

pub(crate) fn trouble(name: &ConfigName, error: &ConfigError) -> Toast {
    Toast::error(format!("Trouble with {name}")).with_text(error.to_string())
}

impl Workspace {
    pub(crate) fn keymap_reloaded(
        &mut self,
        keymap_overrides: KeymapOverrides,
        revisions: &mut Revisions,
    ) -> Result<Cmd, Unhandled> {
        if self.keymap.overrides() == &keymap_overrides {
            return Err(Unhandled);
        }
        self.keymap = Keymap::new(keymap_overrides);
        revisions.config.advance();
        Ok(Cmd::none())
    }

    pub(crate) fn show(&mut self, toast: Toast, revisions: &mut Revisions) -> Cmd {
        let was_empty = self.toasts.is_empty();
        self.toasts.insert(
            0,
            Toast {
                raised_at: self.clock,
                ..toast
            },
        );
        self.toasts.truncate(TOAST_STACK);
        let raised = Cmd::from(Cue::ToastRaised);
        if was_empty {
            raised.then(
                Effect::After {
                    delay: TOAST_LIFETIME,
                    timer: Timer::Toast(revisions.issue_toast()),
                }
                .into(),
            )
        } else {
            raised
        }
    }

    pub(crate) fn dismiss_newest(&mut self) -> Option<Toast> {
        (!self.toasts.is_empty()).then(|| self.toasts.remove(0))
    }

    pub(crate) fn expire_toasts(&mut self, revision: Revision) -> Cmd {
        let now = self.clock;
        let before = self.toasts.len();
        self.toasts
            .retain(|toast| now.elapsed_since(toast.raised_at) < TOAST_LIFETIME);
        let dropped = if self.toasts.len() < before {
            Cmd::from(Cue::ToastDismissed)
        } else {
            Cmd::none()
        };
        let next = self.toasts.last().map_or(Cmd::none(), |oldest| {
            Effect::After {
                delay: TOAST_LIFETIME
                    .saturating_sub(now.elapsed_since(oldest.raised_at)),
                timer: Timer::Toast(revision),
            }
            .into()
        });
        dropped.then(next)
    }

    pub(crate) fn config_reloaded(
        &mut self,
        reload: ConfigReload,
        revisions: &mut Revisions,
    ) -> Result<Cmd, Unhandled> {
        let result = match (&reload.name, reload.result) {
            (ConfigName::Config, Ok(())) => self
                .keymap
                .diagnostic()
                .map_or(Ok(()), |diagnostic| Err(ConfigError::Parse(diagnostic))),
            (
                ConfigName::Config | ConfigName::Appearance | ConfigName::Theme(_),
                result,
            ) => result,
        };
        let reload = ConfigReload { result, ..reload };
        if reload.result.is_ok() && self.config_errors.get(&reload.name).is_none() {
            return Err(Unhandled);
        }
        self.config_reported(reload, revisions)
    }

    fn config_reported(
        &mut self,
        reload: ConfigReload,
        revisions: &mut Revisions,
    ) -> Result<Cmd, Unhandled> {
        let ConfigReload { name, result } = reload;
        match result {
            Err(error) => {
                let toast = trouble(&name, &error);
                self.config_errors.replace(name, error)?;
                Ok(self.show(toast, revisions))
            }
            Ok(()) => Ok(self.config_recovered(&name)),
        }
    }

    fn config_recovered(&mut self, name: &ConfigName) -> Cmd {
        let cleared = self
            .config_errors
            .clear(name)
            .map(|error| error.to_string());
        let before = self.toasts.len();
        self.toasts
            .retain(|toast| cleared.is_none() || toast.text != cleared);
        if self.toasts.len() < before {
            Cue::ToastDismissed.into()
        } else {
            Cmd::none()
        }
    }
}
