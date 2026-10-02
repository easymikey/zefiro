use crate::{
    cmd::{Cmd, Cue, Effect},
    domain::{
        ConfigFile,
        Keymap,
        KeymapOverrides,
        Revision,
        Revisions,
        TOAST_LIFETIME,
        TOAST_STACK,
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
            Cmd::None
        };
        let next = self.toasts.last().map_or(Cmd::None, |oldest| {
            Effect::After {
                delay: TOAST_LIFETIME
                    .saturating_sub(now.elapsed_since(oldest.raised_at)),
                timer: Timer::Toast(revision),
            }
            .into()
        });
        dropped.then(next)
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
                fresh.map_or(Cmd::None, |told| {
                    let title = format!("Trouble with {source}");
                    self.show(Toast::error(title).with_text(told), revisions)
                })
            }
            None => self.source_recovered(source),
        }
    }

    pub(crate) fn source_recovered(&mut self, source: ConfigFile) -> Cmd {
        let cleared = self.source_errors.clear(source);
        let before = self.toasts.len();
        self.toasts
            .retain(|toast| cleared.is_none() || toast.text != cleared);
        if self.toasts.len() < before {
            Cue::ToastDismissed.into()
        } else {
            Cmd::None
        }
    }
}
