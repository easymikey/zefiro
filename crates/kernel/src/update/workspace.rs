use crate::{
    cmd::{Cmd, Cue, Effect},
    domain::{
        ConfigFile,
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
        machine::{Machine, Rejected},
    },
};

pub(crate) struct SourceOutcome {
    pub(crate) source: ConfigFile,
    pub(crate) text: Option<String>,
}

impl Workspace {
    pub(crate) fn keymap_reloaded(&mut self, keys: KeymapOverrides) -> Cmd {
        if self.keymap.config() == &keys {
            return Cmd::None;
        }
        self.keymap = Keymap::new(keys, &default_bindings());
        let text = self.keymap.error_text();
        self.source_result(SourceOutcome {
            source: ConfigFile::Config,
            text,
        })
    }

    pub(crate) fn show(&mut self, toast: Toast) -> Cmd {
        self.toast = Some(toast);
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            Effect::After {
                delay: TOAST_LIFETIME,
                message: Timer::Toast(Revision::UNSTAMPED),
            },
        ])
    }

    pub(crate) fn source_result(&mut self, outcome: SourceOutcome) -> Cmd {
        let SourceOutcome { source, text } = outcome;
        match text {
            Some(text) => {
                let fresh = self.source_errors.insert_if_changed(source, text);
                fresh.map_or(Cmd::None, |told| self.show(Toast::error(told)))
            }
            None => self.source_recovered(source),
        }
    }

    pub(crate) fn source_recovered(&mut self, source: ConfigFile) -> Cmd {
        let cleared = self.source_errors.clear(source);
        self.toast
            .take_if(|toast| Some(toast.text.as_str()) == cleared.as_deref())
            .map_or(Cmd::None, |_| Cue::ToastDismissed.into())
    }
}

impl Machine for Workspace {
    type Message = WorkspaceRequest;
    type Error = std::convert::Infallible;
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
        };
        Ok((self, cmd))
    }
}
