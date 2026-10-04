use crate::{
    cmd::{Cmd, Effect},
    domain::{
        config::{ConfigError, ConfigName},
        cue::Cue,
        keymap::KeymapOverrides,
        revision::{Revision, Revisions},
        toast::{TOAST_LIFETIME, TOAST_STACK, Toast},
        workspace::Workspace,
    },
    message::{ConfigReload, Timer},
    update::keymap::bindings::Keymap,
};

impl Workspace {
    pub(crate) fn keymap_reloaded(
        &mut self,
        keys: KeymapOverrides,
        revisions: &mut Revisions,
    ) -> Cmd {
        if self.keymap.overrides() == &keys {
            return Cmd::none();
        }
        self.keymap = Keymap::new(keys);
        let result = self
            .keymap
            .diagnostic()
            .map_or(Ok(()), |diagnostic| Err(ConfigError::Invalid(diagnostic)));
        self.config_reloaded(
            ConfigReload {
                name: ConfigName::Config,
                result,
            },
            revisions,
        )
    }

    pub(crate) fn show(&mut self, toast: Toast, revisions: &mut Revisions) -> Cmd {
        let first = self.toasts.is_empty();
        self.toasts.insert(
            0,
            Toast {
                raised_at: self.clock,
                ..toast
            },
        );
        self.toasts.truncate(TOAST_STACK);
        let raised = Cmd::from(Cue::ToastRaised);
        if first {
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
        if self.toasts.is_empty() {
            None
        } else {
            Some(self.toasts.remove(0))
        }
    }

    pub(crate) fn expire(&mut self, revision: Revision) -> Cmd {
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
    ) -> Cmd {
        let ConfigReload { name, result } = reload;
        match result {
            Err(error) => {
                let text = error.to_string();
                if self.config_errors.insert_if_changed(name.clone(), error) {
                    let title = format!("Trouble with {name}");
                    self.show(Toast::error(title).with_text(text), revisions)
                } else {
                    Cmd::none()
                }
            }
            Ok(()) => self.source_recovered(&name),
        }
    }

    fn source_recovered(&mut self, name: &ConfigName) -> Cmd {
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
