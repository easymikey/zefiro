use crate::domain::{KeyContext, Overlay, Workspace};

#[must_use]
fn key_context_of(overlay: &Overlay) -> KeyContext {
    match overlay {
        Overlay::Help => KeyContext::Help,
        Overlay::Search(_) => KeyContext::Search,
        Overlay::History(_) => KeyContext::History,
        Overlay::Settings(_) => KeyContext::Settings,
        Overlay::ConfirmDelete(_) => KeyContext::ConfirmDelete,
        Overlay::JumpToTime(_) => KeyContext::JumpToTime,
        Overlay::TrackDetails(_) => KeyContext::TrackDetails,
        Overlay::SavePlaylist { .. } | Overlay::SourceDir { .. } => {
            KeyContext::TextPrompt
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextStack {
    Overlay(KeyContext),
    Base,
}

impl ContextStack {
    #[must_use]
    pub(super) fn primary(self) -> KeyContext {
        match self {
            Self::Overlay(context) => context,
            Self::Base => KeyContext::Playlist,
        }
    }

    #[must_use]
    pub(crate) fn fallback(self) -> Option<KeyContext> {
        match self {
            Self::Overlay(_) => None,
            Self::Base => Some(KeyContext::Global),
        }
    }
}

#[must_use]
pub(crate) fn key_context_stack(workspace: &Workspace) -> ContextStack {
    workspace
        .overlay
        .as_ref()
        .map_or(ContextStack::Base, |overlay| {
            ContextStack::Overlay(key_context_of(overlay))
        })
}
