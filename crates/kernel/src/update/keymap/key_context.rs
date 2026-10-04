use crate::domain::{keymap::KeyContext, overlay::Overlay, workspace::Workspace};

#[must_use]
fn key_context_of(overlay: &Overlay) -> KeyContext {
    match overlay {
        Overlay::Help => KeyContext::Help,
        Overlay::Search(_) => KeyContext::Search,
        Overlay::History(_) => KeyContext::History,
        Overlay::Settings(..) => KeyContext::Settings,
        Overlay::ConfirmDelete(_) => KeyContext::ConfirmDelete,
        Overlay::JumpToTime(_) => KeyContext::JumpToTime,
        Overlay::TrackDetails(_) => KeyContext::TrackDetails,
        Overlay::SavePlaylist { .. } | Overlay::MusicDir { .. } => {
            KeyContext::TextPrompt
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveKeyContext {
    Overlay(KeyContext),
    Base,
}

impl ActiveKeyContext {
    #[must_use]
    pub(crate) fn primary(self) -> KeyContext {
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
pub(crate) fn key_context_stack(workspace: &Workspace) -> ActiveKeyContext {
    workspace
        .overlay
        .as_ref()
        .map_or(ActiveKeyContext::Base, |overlay| {
            ActiveKeyContext::Overlay(key_context_of(overlay))
        })
}
