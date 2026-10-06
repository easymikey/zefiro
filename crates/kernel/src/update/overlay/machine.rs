use std::path::PathBuf;

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
        overlay::{OverlayContentMessage, OverlayMessage},
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
        Overlay::SavePlaylist(text_entry) => confirm_save_playlist(text_entry),
        Overlay::ConfirmDelete(candidate) => Ok(Cmd::message(Message::Browse(
            BrowseRequest::Trash(candidate.source.clone()),
        ))),
        Overlay::JumpToTime(text_entry) => confirm_jump(text_entry),
        Overlay::MusicDir(text_entry) => confirm_music_dir(text_entry),
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

fn confirm_jump(text_entry: &mut TextEntry<TimecodeError>) -> Result<Cmd, Unhandled> {
    match parse_timecode(&text_entry.input) {
        Ok(target) => {
            text_entry.error = None;
            Ok(Cmd::message(Message::Playback(PlaybackRequest::SeekTo(
                target,
            ))))
        }
        Err(error) => {
            let previous = text_entry.error.replace(error);
            (previous != text_entry.error)
                .then(Cmd::none)
                .ok_or(Unhandled)
        }
    }
}

fn confirm_save_playlist(
    text_entry: &mut TextEntry<PlaylistNameError>,
) -> Result<Cmd, Unhandled> {
    match PlaylistFileName::new(&text_entry.input) {
        Ok(name) => {
            text_entry.error = None;
            Ok(Cmd::message(Message::Browse(BrowseRequest::SavePlaylist(
                name,
            ))))
        }
        Err(reason) => {
            let previous = text_entry.error.replace(reason);
            (previous != text_entry.error)
                .then(Cmd::none)
                .ok_or(Unhandled)
        }
    }
}

fn confirm_music_dir(
    text_entry: &mut TextEntry<MusicDirError>,
) -> Result<Cmd, Unhandled> {
    let music_dir = PathBuf::from(text_entry.input.trim());
    if music_dir.as_os_str().is_empty() {
        let previous = text_entry.error.replace(MusicDirError::Empty);
        return (previous != text_entry.error)
            .then(Cmd::none)
            .ok_or(Unhandled);
    }
    text_entry.error = None;
    Ok(Cmd::from(Effect::Config(ConfigCmd::Save(ConfigPatch {
        music_dir: Some(music_dir),
        ..ConfigPatch::default()
    }))))
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
        (Overlay::JumpToTime(text_entry), OverlayContentMessage::Text(message)) => {
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
            | OverlayContentMessage::History(_),
        ) => Err(Unhandled),
    }
}
