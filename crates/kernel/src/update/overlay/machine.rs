use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        cue::Cue,
        cursor_over::CursorOver,
        overlay::{MusicDirError, Overlay, SearchQuery, TextEntry},
        playlist::{PlaylistFileName, PlaylistNameError},
        time::{TimecodeError, parse_timecode},
    },
    message::{BrowseRequest, Message, PlaybackRequest},
    update::{
        machine::{Machine, Unhandled},
        overlay::{OverlayContentMessage, OverlayMessage, jump},
    },
};

impl Machine for Option<Overlay> {
    type Message = OverlayMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: OverlayMessage) -> Result<Cmd, Unhandled> {
        match message {
            OverlayMessage::Open(opened) => {
                let playback = opened_playback(self.as_ref(), &opened);
                *self = Some(opened);
                Ok(Cmd::from(Cue::OverlayOpened).then(playback))
            }
            OverlayMessage::Close => {
                let open = self.take().ok_or(Unhandled)?;
                Ok(cued_close(closed_playback(&open)))
            }
            OverlayMessage::Confirm => confirm(self),
            OverlayMessage::Inner(inner) => inner_transition(self, inner),
        }
    }
}

fn cued_close(cmd: Cmd) -> Cmd {
    cmd.then(Cue::OverlayClosed.into())
}

fn release() -> Cmd {
    Cmd::message(Message::Playback(PlaybackRequest::Release))
}

fn opened_playback(previous: Option<&Overlay>, opened: &Overlay) -> Cmd {
    match (previous, opened) {
        (_, Overlay::Settings(..)) => {
            Cmd::message(Message::Playback(PlaybackRequest::HoldForOverlay))
        }
        (Some(Overlay::Settings(..)), _) => release(),
        (_, _) => Cmd::none(),
    }
}

fn closed_playback(open: &Overlay) -> Cmd {
    match open {
        Overlay::Settings(..) => release(),
        Overlay::Help
        | Overlay::Search(_)
        | Overlay::SavePlaylist(_)
        | Overlay::History(_)
        | Overlay::ConfirmDelete(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir(_) => Cmd::none(),
    }
}

fn confirm(state: &mut Option<Overlay>) -> Result<Cmd, Unhandled> {
    let open = state.as_mut().ok_or(Unhandled)?;
    let cmd = confirmed(open)?;
    if has_error(open) {
        return Ok(Cmd::none());
    }
    *state = None;
    Ok(cued_close(cmd))
}

fn has_error(open: &Overlay) -> bool {
    match open {
        Overlay::SavePlaylist(text_entry) => text_entry.error.is_some(),
        Overlay::MusicDir(text_entry) => text_entry.error.is_some(),
        Overlay::JumpToTime(text_entry) => text_entry.error.is_some(),
        Overlay::Search(_)
        | Overlay::ConfirmDelete(_)
        | Overlay::Settings(..)
        | Overlay::Help
        | Overlay::TrackDetails(_)
        | Overlay::History(_) => false,
    }
}

fn confirmed(open: &mut Overlay) -> Result<Cmd, Unhandled> {
    match open {
        Overlay::Search(search) => confirm_search(search),
        Overlay::SavePlaylist(text_entry) => Ok(confirm_save_playlist(text_entry)),
        Overlay::ConfirmDelete(candidate) => Ok(Cmd::message(Message::Browse(
            BrowseRequest::Trash(candidate.source.clone()),
        ))),
        Overlay::JumpToTime(text_entry) => Ok(confirm_jump(text_entry)),
        Overlay::MusicDir(text_entry) => Ok(confirm_music_dir(text_entry)),
        Overlay::Settings(..) => Ok(release()),
        Overlay::Help | Overlay::TrackDetails(_) | Overlay::History(_) => {
            Err(Unhandled)
        }
    }
}

fn confirm_search(search: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search.selected_match().ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Playback(PlaybackRequest::JumpTo(
        index,
    ))))
}

fn confirm_jump(text_entry: &mut TextEntry<TimecodeError>) -> Cmd {
    match parse_timecode(&text_entry.input) {
        Ok(target) => {
            text_entry.error = None;
            Cmd::message(Message::Playback(PlaybackRequest::SeekTo(target)))
        }
        Err(error) => {
            text_entry.error = Some(error);
            Cmd::none()
        }
    }
}

fn confirm_save_playlist(text_entry: &mut TextEntry<PlaylistNameError>) -> Cmd {
    match PlaylistFileName::new(&text_entry.input) {
        Ok(name) => {
            text_entry.error = None;
            Cmd::message(Message::Browse(BrowseRequest::SavePlaylist(name)))
        }
        Err(reason) => {
            text_entry.error = Some(reason);
            Cmd::none()
        }
    }
}

fn confirm_music_dir(text_entry: &mut TextEntry<MusicDirError>) -> Cmd {
    if text_entry.input.trim().is_empty() {
        text_entry.error = Some(MusicDirError::Empty);
        return Cmd::none();
    }
    text_entry.error = None;
    Cmd::from(Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder()
            .music_dir(std::path::PathBuf::from(text_entry.input.as_str()))
            .build(),
    )))
}

fn inner_transition(
    state: &mut Option<Overlay>,
    inner: OverlayContentMessage,
) -> Result<Cmd, Unhandled> {
    let open = state.as_mut().ok_or(Unhandled)?;
    match (open, inner) {
        (Overlay::Search(search), OverlayContentMessage::Search(message)) => {
            search.transition(message)
        }
        (Overlay::Settings(selected), OverlayContentMessage::Settings(message)) => {
            selected.transition(message)
        }
        (Overlay::SavePlaylist(text_entry), OverlayContentMessage::Text(message)) => {
            text_entry.transition(message)
        }
        (Overlay::MusicDir(text_entry), OverlayContentMessage::Text(message)) => {
            text_entry.transition(message)
        }
        (Overlay::JumpToTime(text_entry), OverlayContentMessage::Jump(message)) => {
            if !jump::admits(text_entry, message) {
                return Err(Unhandled);
            }
            text_entry.transition(message)
        }
        (Overlay::History(cursor), OverlayContentMessage::History(message)) => {
            cursor.transition(message)
        }
        (
            _,
            OverlayContentMessage::Search(_)
            | OverlayContentMessage::Settings(_)
            | OverlayContentMessage::Text(_)
            | OverlayContentMessage::Jump(_)
            | OverlayContentMessage::History(_),
        ) => Err(Unhandled),
    }
}
