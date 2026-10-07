use std::path::PathBuf;

use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        cue::Cue,
        cursor_over::CursorOver,
        overlay::{MusicDirError, Overlay, SearchQuery, TextEntry},
        playlist::{PlaylistFileName, PlaylistFileNameError},
        time::{TimecodeError, parse_timecode},
    },
    message::{BrowseRequest, Message, PlaybackRequest},
    update::{
        machine::{Machine, Unhandled, replace},
        overlay::{OverlayContentMessage, OverlayMessage},
    },
};

impl Machine for Option<Overlay> {
    type Message = OverlayMessage;
    type Effect = Cmd;

    fn transition(
        &mut self,
        overlay_message: OverlayMessage,
    ) -> Result<Cmd, Unhandled> {
        match overlay_message {
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
            OverlayMessage::Content(inner) => content_transition(self, inner),
        }
    }
}

fn cued_close(cmd: Cmd) -> Cmd {
    cmd.then(Cue::OverlayClosed.into())
}

fn release() -> Cmd {
    Cmd::message(Message::Playback(PlaybackRequest::Release))
}

fn opened_playback(previous: Option<&Overlay>, overlay: &Overlay) -> Cmd {
    match overlay {
        Overlay::Settings(..) => {
            Cmd::message(Message::Playback(PlaybackRequest::HoldForOverlay))
        }
        Overlay::Help
        | Overlay::Search(_)
        | Overlay::SavePlaylist(_)
        | Overlay::History(_)
        | Overlay::ConfirmTrash(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir(_) => previous.map_or(Cmd::none(), closed_playback),
    }
}

fn closed_playback(overlay: &Overlay) -> Cmd {
    match overlay {
        Overlay::Settings(..) => release(),
        Overlay::Help
        | Overlay::Search(_)
        | Overlay::SavePlaylist(_)
        | Overlay::History(_)
        | Overlay::ConfirmTrash(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir(_) => Cmd::none(),
    }
}

enum Confirmed {
    Close(Cmd),
    Stay,
}

fn confirm(overlay: &mut Option<Overlay>) -> Result<Cmd, Unhandled> {
    let open = overlay.as_mut().ok_or(Unhandled)?;
    match confirm_cmd(open)? {
        Confirmed::Close(cmd) => {
            *overlay = None;
            Ok(cued_close(cmd))
        }
        Confirmed::Stay => Ok(Cmd::none()),
    }
}

fn confirm_cmd(overlay: &mut Overlay) -> Result<Confirmed, Unhandled> {
    match overlay {
        Overlay::Search(search) => confirm_search(search).map(Confirmed::Close),
        Overlay::SavePlaylist(text_entry) => confirm_save_playlist(text_entry),
        Overlay::ConfirmTrash(track) => Ok(Confirmed::Close(Cmd::message(
            Message::Browse(BrowseRequest::Trash(track.source().clone())),
        ))),
        Overlay::JumpToTime(text_entry) => confirm_jump(text_entry),
        Overlay::MusicDir(text_entry) => confirm_music_dir(text_entry),
        Overlay::Settings(..)
        | Overlay::Help
        | Overlay::TrackDetails(_)
        | Overlay::History(_) => Err(Unhandled),
    }
}

fn confirm_search(search_query: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search_query.selected_match().ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Playback(PlaybackRequest::JumpTo(
        index,
    ))))
}

fn confirm_jump(
    text_entry: &mut TextEntry<TimecodeError>,
) -> Result<Confirmed, Unhandled> {
    match parse_timecode(&text_entry.input) {
        Ok(target) => Ok(Confirmed::Close(Cmd::message(Message::Playback(
            PlaybackRequest::SeekTo(target),
        )))),
        Err(error) => {
            replace(&mut text_entry.error, Some(error)).map(|()| Confirmed::Stay)
        }
    }
}

fn confirm_save_playlist(
    text_entry: &mut TextEntry<PlaylistFileNameError>,
) -> Result<Confirmed, Unhandled> {
    match PlaylistFileName::new(&text_entry.input) {
        Ok(name) => Ok(Confirmed::Close(Cmd::message(Message::Browse(
            BrowseRequest::SavePlaylist(name),
        )))),
        Err(reason) => {
            replace(&mut text_entry.error, Some(reason)).map(|()| Confirmed::Stay)
        }
    }
}

fn confirm_music_dir(
    text_entry: &mut TextEntry<MusicDirError>,
) -> Result<Confirmed, Unhandled> {
    let music_dir = PathBuf::from(text_entry.input.trim());
    if music_dir.as_os_str().is_empty() {
        return replace(&mut text_entry.error, Some(MusicDirError::Empty))
            .map(|()| Confirmed::Stay);
    }
    Ok(Confirmed::Close(Cmd::from(Effect::Config(
        ConfigCmd::Save(ConfigPatch {
            music_dir: Some(music_dir),
            ..ConfigPatch::default()
        }),
    ))))
}

fn content_transition(
    overlay: &mut Option<Overlay>,
    content_message: OverlayContentMessage,
) -> Result<Cmd, Unhandled> {
    let open = overlay.as_mut().ok_or(Unhandled)?;
    match (open, content_message) {
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
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::SavePlaylist(_)
            | Overlay::History(_)
            | Overlay::Settings(_)
            | Overlay::ConfirmTrash(_)
            | Overlay::TrackDetails(_)
            | Overlay::JumpToTime(_)
            | Overlay::MusicDir(_),
            OverlayContentMessage::Search(_)
            | OverlayContentMessage::Settings(_)
            | OverlayContentMessage::Text(_)
            | OverlayContentMessage::History(_),
        ) => Err(Unhandled),
    }
}
