use crate::domain::{keymap::KeyContext, overlay::Overlay};

#[must_use]
pub(crate) fn key_context_of(overlay: &Overlay) -> KeyContext {
    match overlay {
        Overlay::Help => KeyContext::Help,
        Overlay::Search(_) | Overlay::ServerSearch(_) => KeyContext::Search,
        Overlay::History(_) => KeyContext::History,
        Overlay::Settings(..) => KeyContext::Settings,
        Overlay::ConfirmTrash(_) => KeyContext::ConfirmTrash,
        Overlay::JumpToTime(_) => KeyContext::JumpToTime,
        Overlay::TrackDetails(_) => KeyContext::TrackDetails,
        Overlay::Servers(_) => KeyContext::Servers,
        Overlay::ConfirmRemove(_) => KeyContext::ConfirmRemove,
        Overlay::SavePlaylist(_) | Overlay::MusicDir(_) | Overlay::AddServer(_) => {
            KeyContext::TextPrompt
        }
    }
}
