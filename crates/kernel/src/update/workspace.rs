use crate::{
    cmd::{Cmd, Cue, Effect},
    domain::{
        ConfigSource,
        KeyValidationErrors,
        Keymap,
        KeymapOverrides,
        Revision,
        TOAST_LIFETIME,
        Toast,
        Workspace,
    },
    message::{Timer, WorkspaceRequest},
    update::{
        keymap::default_bindings,
        machine::{Machine, Never, Rejected},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeymapReload {
    Fresh,
    Unchanged,
}

struct SourceOutcome {
    source: ConfigSource,
    text: Option<String>,
}

impl Workspace {
    pub(crate) fn keymap_reload(&self, keys: &KeymapOverrides) -> KeymapReload {
        if self.keymap.config() == keys {
            KeymapReload::Unchanged
        } else {
            KeymapReload::Fresh
        }
    }

    fn keymap_reloaded(&mut self, keys: KeymapOverrides) -> Cmd {
        match self.keymap_reload(&keys) {
            KeymapReload::Unchanged => Cmd::None,
            KeymapReload::Fresh => {
                self.keymap = Keymap::new(keys, &default_bindings());
                let text = (!self.keymap.errors().is_empty()).then(|| {
                    KeyValidationErrors(self.keymap.errors().to_vec()).to_string()
                });
                self.source_result(SourceOutcome {
                    source: ConfigSource::Keymap,
                    text,
                })
            }
        }
    }

    fn show(&mut self, toast: Toast) -> Cmd {
        self.toast = Some(toast);
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            Effect::After {
                delay: TOAST_LIFETIME,
                message: Timer::Toast(Revision::UNSTAMPED),
            },
        ])
    }

    fn source_result(&mut self, outcome: SourceOutcome) -> Cmd {
        let SourceOutcome { source, text } = outcome;
        match text {
            Some(text) => {
                let fresh = self.source_errors.note(source, text);
                fresh.map_or(Cmd::None, |told| self.show(Toast::error(told)))
            }
            None => self.source_recovered(source),
        }
    }

    fn source_recovered(&mut self, source: ConfigSource) -> Cmd {
        let cleared = self.source_errors.clear(source);
        self.toast
            .take_if(|toast| Some(toast.text.as_str()) == cleared.as_deref())
            .map_or(Cmd::None, |_| Cue::ToastDismissed.into())
    }
}

impl Machine for Workspace {
    type Message = WorkspaceRequest;
    type Rejection = Never;
    type Effect = Cmd;

    fn transition(
        mut self,
        request: WorkspaceRequest,
    ) -> Result<(Self, Cmd), Rejected<Self>> {
        let cmd = match request {
            WorkspaceRequest::ShowToast(toast) => self.show(toast),
            WorkspaceRequest::ClearToast => {
                self.toast = None;
                Cmd::None
            }
            WorkspaceRequest::KeymapReloaded(keys) => self.keymap_reloaded(*keys),
            WorkspaceRequest::ThemeReloaded => Cmd::None,
            WorkspaceRequest::SourceFailed { source, text } => {
                self.source_result(SourceOutcome {
                    source,
                    text: Some(text),
                })
            }
            WorkspaceRequest::SourceRecovered(source) => self.source_recovered(source),
            WorkspaceRequest::ConfigFailed(failure) => {
                self.show(Toast::error(failure.to_string()))
            }
        };
        Ok((self, cmd))
    }
}
