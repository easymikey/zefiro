use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Effect},
    domain::{
        cue::Cue,
        cursor_over::CursorOver,
        direction::Direction,
        overlay::{
            Field,
            MusicDirError,
            Overlay,
            SearchQuery,
            ServerPrompt,
            TextEntry,
            Verdict,
        },
        playlist::{PlaylistFileName, PlaylistFileNameError},
        server::{
            Account,
            Connection,
            Credential,
            Endpoint,
            RemoteError,
            Secret,
            ServerName,
            ServerStatus,
            UserName,
        },
        time::{TimecodeError, parse_timecode},
    },
    message::{
        BrowseRequest,
        ConfigEvent,
        Message,
        OverlayRequest,
        PlaybackRequest,
        ServerRequest,
        TextRequest,
    },
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
        | Overlay::ServerSearch
        | Overlay::SavePlaylist(_)
        | Overlay::History(_)
        | Overlay::ConfirmTrash(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir { .. }
        | Overlay::AddServer(_)
        | Overlay::Servers(_)
        | Overlay::ConfirmRemove(_) => previous.map_or(Cmd::none(), closed_playback),
    }
}

fn closed_playback(overlay: &Overlay) -> Cmd {
    match overlay {
        Overlay::Settings(..) => release(),
        Overlay::Help
        | Overlay::Search(_)
        | Overlay::ServerSearch
        | Overlay::SavePlaylist(_)
        | Overlay::History(_)
        | Overlay::ConfirmTrash(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir { .. }
        | Overlay::AddServer(_)
        | Overlay::Servers(_)
        | Overlay::ConfirmRemove(_) => Cmd::none(),
    }
}

enum Confirmed {
    Close(Cmd),
    Stay(Cmd),
}

fn confirm(overlay: &mut Option<Overlay>) -> Result<Cmd, Unhandled> {
    let open = overlay.as_mut().ok_or(Unhandled)?;
    match confirm_cmd(open)? {
        Confirmed::Close(cmd) => {
            *overlay = None;
            Ok(cued_close(cmd))
        }
        Confirmed::Stay(cmd) => Ok(cmd),
    }
}

fn confirm_cmd(overlay: &mut Overlay) -> Result<Confirmed, Unhandled> {
    match overlay {
        Overlay::Search(search) => confirm_search(search).map(Confirmed::Close),
        Overlay::ServerSearch => Ok(Confirmed::Close(Cmd::none())),
        Overlay::SavePlaylist(text_entry) => confirm_save_playlist(text_entry),
        Overlay::ConfirmTrash(track) => Ok(Confirmed::Close(Cmd::message(
            Message::Browse(BrowseRequest::Trash(track.source().clone())),
        ))),
        Overlay::JumpToTime(text_entry) => confirm_jump(text_entry),
        Overlay::MusicDir {
            text_entry,
            verdict,
            revision,
            folders: _,
        } => confirm_music_dir(text_entry, verdict.filter(|_| revision.is_none())),
        Overlay::AddServer(server_prompt) => server_prompt.confirm(),
        Overlay::ConfirmRemove(server_name) => Ok(Confirmed::Close(Cmd::message(
            Message::Server(ServerRequest::Remove(server_name.clone())),
        ))),
        Overlay::Settings(..)
        | Overlay::Help
        | Overlay::TrackDetails(_)
        | Overlay::History(_)
        | Overlay::Servers(_) => Err(Unhandled),
    }
}

fn confirm_search(search_query: &CursorOver<SearchQuery>) -> Result<Cmd, Unhandled> {
    let index = search_query.selected_match().ok_or(Unhandled)?;
    Ok(Cmd::message(Message::Browse(BrowseRequest::JumpTo(index))))
}

fn confirm_jump(
    text_entry: &mut TextEntry<TimecodeError>,
) -> Result<Confirmed, Unhandled> {
    match parse_timecode(&text_entry.input) {
        Ok(target) => Ok(Confirmed::Close(Cmd::message(Message::Playback(
            PlaybackRequest::SeekTo(target),
        )))),
        Err(error) => replace(&mut text_entry.error, Some(error))
            .map(|()| Confirmed::Stay(Cmd::none())),
    }
}

fn confirm_save_playlist(
    text_entry: &mut TextEntry<PlaylistFileNameError>,
) -> Result<Confirmed, Unhandled> {
    match PlaylistFileName::new(&text_entry.input) {
        Ok(name) => Ok(Confirmed::Close(Cmd::message(Message::Browse(
            BrowseRequest::SavePlaylist(name),
        )))),
        Err(reason) => replace(&mut text_entry.error, Some(reason))
            .map(|()| Confirmed::Stay(Cmd::none())),
    }
}

fn confirm_music_dir(
    text_entry: &mut TextEntry<MusicDirError>,
    verdict: Option<Verdict>,
) -> Result<Confirmed, Unhandled> {
    let music_dir = text_entry.path();
    if music_dir.as_os_str().is_empty() {
        return replace(&mut text_entry.error, Some(MusicDirError::Empty))
            .map(|()| Confirmed::Stay(Cmd::none()));
    }
    match verdict {
        Some(Verdict::Readable) => {}
        Some(
            Verdict::Denied
            | Verdict::Missing
            | Verdict::NotADirectory
            | Verdict::Unreadable(_),
        )
        | None => {
            return replace(&mut text_entry.error, Some(MusicDirError::Pending))
                .map(|()| Confirmed::Stay(Cmd::none()));
        }
    }
    let save = Cmd::from(Effect::Config(ConfigCmd::Save(ConfigPatch {
        music_dir: Some(music_dir.clone()),
        ..ConfigPatch::default()
    })));
    Ok(Confirmed::Close(save.then(Cmd::message(Message::Config(
        ConfigEvent::MusicDirReloaded(music_dir),
    )))))
}

impl ServerPrompt {
    fn connecting(&self) -> Result<(), Unhandled> {
        match self.server_status {
            Some(ServerStatus::Connecting) => Err(Unhandled),
            Some(ServerStatus::Online(_) | ServerStatus::Offline(_)) | None => Ok(()),
        }
    }

    pub(crate) fn leave(&mut self, direction: Direction) -> Result<(), Unhandled> {
        self.connecting()?;
        let next = match (self.field, direction) {
            (Field::Link, Direction::Next) | (Field::Password, Direction::Previous) => {
                Field::User
            }
            (Field::User, Direction::Next) => Field::Password,
            (Field::User, Direction::Previous) => Field::Link,
            (Field::Link, Direction::Previous) | (Field::Password, Direction::Next) => {
                return Err(Unhandled);
            }
        };
        match self.field {
            Field::Link => {
                self.link_text_entry.error =
                    Endpoint::parse(&self.link_text_entry.input).err();
            }
            Field::User => {
                self.user_text_entry.error =
                    UserName::new(&self.user_text_entry.input).err();
            }
            Field::Password => {
                self.password_text_entry.error =
                    Secret::new(&self.password_text_entry.input).err();
            }
        }
        self.field = next;
        self.reached_field = self.reached_field.max(next);
        Ok(())
    }

    fn confirm(&mut self) -> Result<Confirmed, Unhandled> {
        match self.field {
            Field::Link | Field::User => {
                self.leave(Direction::Next)?;
                return Ok(Confirmed::Stay(Cmd::none()));
            }
            Field::Password => self.connecting()?,
        }
        match (
            Endpoint::parse(&self.link_text_entry.input),
            UserName::new(&self.user_text_entry.input),
            Secret::new(&self.password_text_entry.input),
        ) {
            (Ok(endpoint), Ok(user_name), Ok(secret)) => {
                let server_name = ServerName::new(endpoint.authority());
                self.server_status = Some(ServerStatus::Connecting);
                let origin_server_name =
                    self.origin_server_name.replace(server_name.clone());
                Ok(Confirmed::Stay(Cmd::message(Message::Server(
                    ServerRequest::Add {
                        connection: Connection {
                            account: Account {
                                server_name,
                                endpoint,
                                user_name,
                            },
                            credential: Credential::Typed(secret),
                        },
                        origin_server_name,
                    },
                ))))
            }
            (link, user, password) => {
                let field = match (&link, &user) {
                    (Err(_), _) => Field::Link,
                    (Ok(_), Err(_)) => Field::User,
                    (Ok(_), Ok(_)) => Field::Password,
                };
                let link = link.err();
                let user = user.err();
                let password = password.err();
                if field == self.field
                    && link == self.link_text_entry.error
                    && user == self.user_text_entry.error
                    && password == self.password_text_entry.error
                {
                    return Err(Unhandled);
                }
                self.field = field;
                self.link_text_entry.error = link;
                self.user_text_entry.error = user;
                self.password_text_entry.error = password;
                Ok(Confirmed::Stay(Cmd::none()))
            }
        }
    }

    pub(crate) fn answered(
        &mut self,
        server_name: &ServerName,
        server_status: &ServerStatus,
    ) -> Option<Cmd> {
        let Some(ServerStatus::Connecting) = self.server_status else {
            return None;
        };
        Endpoint::parse(&self.link_text_entry.input)
            .ok()
            .filter(|endpoint| ServerName::new(endpoint.authority()) == *server_name)?;
        match server_status {
            ServerStatus::Online(_) => {
                Some(Cmd::message(Message::Overlay(OverlayRequest::Close)))
            }
            ServerStatus::Connecting => None,
            ServerStatus::Offline(error) => {
                self.field = match error {
                    RemoteError::Api { .. }
                    | RemoteError::NoPassword { .. }
                    | RemoteError::Keychain { .. } => Field::Password,
                    RemoteError::Unreachable { .. }
                    | RemoteError::Status { .. }
                    | RemoteError::Moved { .. }
                    | RemoteError::Parse { .. }
                    | RemoteError::Cache { .. } => Field::Link,
                };
                self.server_status = Some(ServerStatus::Offline(error.clone()));
                Some(Cmd::none())
            }
        }
    }
}

impl Machine for ServerPrompt {
    type Message = TextRequest;
    type Effect = Cmd;

    fn transition(&mut self, message: TextRequest) -> Result<Cmd, Unhandled> {
        self.connecting()?;
        match self.field {
            Field::Link => {
                let cmd = self.link_text_entry.transition(message)?;
                if self.reached_field > Field::Link {
                    self.link_text_entry.error =
                        Endpoint::parse(&self.link_text_entry.input).err();
                }
                Ok(cmd)
            }
            Field::User => self.user_text_entry.transition(message),
            Field::Password => self.password_text_entry.transition(message),
        }
    }
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
        (
            Overlay::MusicDir { text_entry, .. },
            OverlayContentMessage::Text(message),
        ) => text_entry.transition(message),
        (Overlay::JumpToTime(text_entry), OverlayContentMessage::Text(message)) => {
            text_entry.transition(message)
        }
        (Overlay::AddServer(server_prompt), OverlayContentMessage::Text(message)) => {
            server_prompt.transition(message)
        }
        (Overlay::History(cursor), OverlayContentMessage::History(message)) => {
            cursor.transition(message)
        }
        (
            Overlay::Help
            | Overlay::Search(_)
            | Overlay::ServerSearch
            | Overlay::SavePlaylist(_)
            | Overlay::History(_)
            | Overlay::Settings(_)
            | Overlay::ConfirmTrash(_)
            | Overlay::TrackDetails(_)
            | Overlay::JumpToTime(_)
            | Overlay::MusicDir { .. }
            | Overlay::AddServer(_)
            | Overlay::Servers(_)
            | Overlay::ConfirmRemove(_),
            OverlayContentMessage::Search(_)
            | OverlayContentMessage::Settings(_)
            | OverlayContentMessage::Text(_)
            | OverlayContentMessage::History(_),
        ) => Err(Unhandled),
    }
}
