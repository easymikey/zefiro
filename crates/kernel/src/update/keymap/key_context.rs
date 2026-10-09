use crate::domain::{keymap::KeyContext, overlay::Overlay};

#[must_use]
pub(crate) fn key_context_of(overlay: &Overlay) -> KeyContext {
    match overlay {
        Overlay::Help => KeyContext::Help,
        Overlay::Search(_) | Overlay::ServerSearch => KeyContext::Search,
        Overlay::History(_) => KeyContext::History,
        Overlay::Settings(..) => KeyContext::Settings,
        Overlay::ConfirmTrash(_) => KeyContext::ConfirmTrash,
        Overlay::JumpToTime(_) => KeyContext::JumpToTime,
        Overlay::TrackDetails(_) => KeyContext::TrackDetails,
        Overlay::Servers(_) => KeyContext::Servers,
        Overlay::ConfirmRemove(_) => KeyContext::ConfirmRemove,
        Overlay::MusicDir { .. } => KeyContext::MusicDir,
        Overlay::SavePlaylist(_) | Overlay::AddServer(_) => KeyContext::TextPrompt,
    }
}
