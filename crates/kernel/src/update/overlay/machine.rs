use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        cue::Cue,
        cursor_over::CursorOver,
        overlay::{JumpDigits, MusicDirError, Overlay, SearchQuery, TextEntry},
        playlist::{PlaylistFileName, PlaylistNameError},
        time::parse_timecode,
    },
    message::{BrowseRequest, Message, PlaybackRequest, PlaylistRequest},
    update::{
        machine::{Machine, Unhandled},
        overlay::{OverlayContentMessage, OverlayMessage, text},
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
        | Overlay::SavePlaylist { .. }
        | Overlay::History(_)
        | Overlay::ConfirmDelete(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir { .. } => Cmd::none(),
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
        Overlay::SavePlaylist { error, .. } => error.is_some(),
        Overlay::MusicDir { error, .. } => error.is_some(),
        Overlay::JumpToTime(digits) => digits.error.is_some(),
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
        Overlay::SavePlaylist { typed, error } => {
            Ok(confirm_save_playlist(typed, error))
        }
        Overlay::ConfirmDelete(candidate) => Ok(Cmd::message(Message::Browse(
            BrowseRequest::Trash(candidate.source.clone()),
        ))),
        Overlay::JumpToTime(digits) => Ok(confirm_jump(digits)),
        Overlay::MusicDir { typed, error } => Ok(confirm_music_dir(typed, error)),
        Overlay::Settings(..) => Ok(release()),
        Overlay::Help | Overlay::TrackDetails(_) | Overlay::History(_) => {
            Err(Unhandled)
        }
    }
}

fn confirm_search(search: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search
        .content
        .matches
        .get(search.selected().get())
        .copied()
        .ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Playlist(PlaylistRequest::JumpTo(
        index,
    ))))
}

fn confirm_jump(digits: &mut JumpDigits) -> Cmd {
    match parse_timecode(&digits.input) {
        Ok(target) => {
            digits.error = None;
            Cmd::message(Message::Playback(PlaybackRequest::SeekTo(target)))
        }
        Err(error) => {
            digits.error = Some(error);
            Cmd::none()
        }
    }
}

fn confirm_save_playlist(
    typed: &TextEntry,
    error: &mut Option<PlaylistNameError>,
) -> Cmd {
    match PlaylistFileName::new(&typed.input) {
        Ok(name) => {
            *error = None;
            Cmd::message(Message::Browse(BrowseRequest::SavePlaylist(name)))
        }
        Err(reason) => {
            *error = Some(reason);
            Cmd::none()
        }
    }
}

fn confirm_music_dir(typed: &TextEntry, error: &mut Option<MusicDirError>) -> Cmd {
    if typed.input.trim().is_empty() {
        *error = Some(MusicDirError::Empty);
        return Cmd::none();
    }
    *error = None;
    Cmd::from(Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder()
            .music_dir(std::path::PathBuf::from(typed.input.as_str()))
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
        (
            Overlay::SavePlaylist { typed, error },
            OverlayContentMessage::Text(message),
        ) => {
            text::retype(typed, message);
            *error = None;
            Ok(Cmd::none())
        }
        (Overlay::MusicDir { typed, error }, OverlayContentMessage::Text(message)) => {
            text::retype(typed, message);
            *error = None;
            Ok(Cmd::none())
        }
        (Overlay::JumpToTime(digits), OverlayContentMessage::Jump(message)) => {
            digits.transition(message)
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
