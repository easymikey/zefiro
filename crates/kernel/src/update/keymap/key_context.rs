use crate::domain::{keymap::KeyContext, overlay::Overlay};

#[must_use]
pub(crate) fn key_context_of(overlay: &Overlay) -> KeyContext {
    match overlay {
        Overlay::Help => KeyContext::Help,
        Overlay::Search(_) => KeyContext::Search,
        Overlay::History(_) => KeyContext::History,
        Overlay::Settings(..) => KeyContext::Settings,
        Overlay::ConfirmDelete(_) => KeyContext::ConfirmDelete,
        Overlay::JumpToTime(_) => KeyContext::JumpToTime,
        Overlay::TrackDetails(_) => KeyContext::TrackDetails,
        Overlay::SavePlaylist(_) | Overlay::MusicDir(_) => KeyContext::TextPrompt,
    }
}
