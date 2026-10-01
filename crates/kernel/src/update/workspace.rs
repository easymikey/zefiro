use crate::{
    cmd::{Cmd, Cue, Effect},
    domain::{
        ConfigFile,
        Keymap,
        KeymapOverrides,
        Revisions,
        TOAST_LIFETIME,
        Toast,
        Workspace,
    },
    message::Timer,
};

pub(crate) struct SourceOutcome {
    pub(crate) source: ConfigFile,
    pub(crate) text: Option<String>,
}

impl Workspace {
    pub(crate) fn keymap_reloaded(
        &mut self,
        keys: KeymapOverrides,
        revisions: &mut Revisions,
    ) -> Cmd {
        if self.keymap.overrides() == &keys {
            return Cmd::None;
        }
        self.keymap = Keymap::new(keys);
        let text = self.keymap.error_text();
        self.source_result(
            SourceOutcome {
                source: ConfigFile::Config,
                text,
            },
            revisions,
        )
    }

    pub(crate) fn show(&mut self, toast: Toast, revisions: &mut Revisions) -> Cmd {
        self.toast = Some(toast);
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            Effect::After {
                delay: TOAST_LIFETIME,
                message: Timer::Toast(revisions.issue_toast()),
            },
        ])
    }

    pub(crate) fn source_result(
        &mut self,
        outcome: SourceOutcome,
        revisions: &mut Revisions,
    ) -> Cmd {
        let SourceOutcome { source, text } = outcome;
        match text {
            Some(text) => {
                let fresh = self.source_errors.insert_if_changed(source, text);
                fresh.map_or(Cmd::None, |told| self.show(Toast::error(told), revisions))
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
