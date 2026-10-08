use std::{fmt, sync::Arc};

use strum::{EnumDiscriminants, EnumIter, IntoStaticStr};

use crate::domain::{
    cursor_over::CursorOver,
    index::ViewIndex,
    playlist::PlaylistFileNameError,
    revision::Revision,
    server::{
        Endpoint,
        EndpointError,
        SecretError,
        ServerName,
        UserName,
        UserNameError,
    },
    setting_row::SettingRow,
    time::TimecodeError,
    track::{CatalogRow, Track},
};

#[derive(Debug, Clone, PartialEq, IntoStaticStr, EnumDiscriminants)]
#[strum(serialize_all = "snake_case")]
#[strum_discriminants(
    name(OverlayName),
    derive(IntoStaticStr, EnumIter),
    strum(serialize_all = "snake_case")
)]
pub enum Overlay {
    Help,
    Search(CursorOver<SearchQuery>),
    ServerSearch(CursorOver<ServerQuery>),
    SavePlaylist(TextEntry<PlaylistFileNameError>),
    History(CursorOver<()>),
    Settings(SettingRow),
    ConfirmTrash(Arc<Track>),
    JumpToTime(TextEntry<TimecodeError>),
    TrackDetails(Arc<Track>),
    MusicDir(TextEntry<MusicDirError>),
    AddServer(ServerPrompt),
    Servers(CursorOver<()>),
    ConfirmRemove(ServerName),
}

impl Overlay {
    #[must_use]
    pub(crate) fn captures_text(&self) -> bool {
        match self {
            Overlay::Search(_)
            | Overlay::ServerSearch(_)
            | Overlay::SavePlaylist(_)
            | Overlay::MusicDir(_)
            | Overlay::AddServer(_) => true,
            Overlay::Help
            | Overlay::History(_)
            | Overlay::Settings(..)
            | Overlay::ConfirmTrash(_)
            | Overlay::JumpToTime(_)
            | Overlay::TrackDetails(_)
            | Overlay::Servers(_)
            | Overlay::ConfirmRemove(_) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEntry<E> {
    pub input: String,
    pub error: Option<E>,
}

impl<E> Default for TextEntry<E> {
    fn default() -> Self {
        Self {
            input: String::new(),
            error: None,
        }
    }
}

pub trait Accepts {
    const MAX_LEN: usize;

    #[must_use]
    fn accepts(character: char) -> bool;
}

impl Accepts for PlaylistFileNameError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(_character: char) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MusicDirError {
    #[error("enter a folder path")]
    Empty,
}

impl Accepts for MusicDirError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(_character: char) -> bool {
        true
    }
}

fn printable(character: char) -> bool {
    !character.is_whitespace() && !character.is_control()
}

impl Accepts for EndpointError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(character: char) -> bool {
        printable(character)
    }
}

impl Accepts for UserNameError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(character: char) -> bool {
        printable(character)
    }
}

impl Accepts for SecretError {
    const MAX_LEN: usize = usize::MAX;

    fn accepts(_character: char) -> bool {
        true
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ServerPrompt {
    Link {
        origin_server_name: Option<ServerName>,
        text_entry: TextEntry<EndpointError>,
    },
    User {
        origin_server_name: Option<ServerName>,
        endpoint: Endpoint,
        text_entry: TextEntry<UserNameError>,
    },
    Password {
        origin_server_name: Option<ServerName>,
        endpoint: Endpoint,
        user_name: UserName,
        text_entry: TextEntry<SecretError>,
    },
}

impl fmt::Debug for ServerPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link {
                origin_server_name,
                text_entry,
            } => formatter
                .debug_struct("Link")
                .field("origin_server_name", origin_server_name)
                .field("text_entry", text_entry)
                .finish(),
            Self::User {
                origin_server_name,
                endpoint,
                text_entry,
            } => formatter
                .debug_struct("User")
                .field("origin_server_name", origin_server_name)
                .field("endpoint", endpoint)
                .field("text_entry", text_entry)
                .finish(),
            Self::Password {
                origin_server_name,
                endpoint,
                user_name,
                text_entry,
            } => formatter
                .debug_struct("Password")
                .field("origin_server_name", origin_server_name)
                .field("endpoint", endpoint)
                .field("user_name", user_name)
                .field("error", &text_entry.error)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub input: String,
    pub matches: Vec<ViewIndex>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerQuery {
    pub server_name: ServerName,
    pub input: String,
    pub catalog_rows: Vec<CatalogRow>,
    pub revision: Option<Revision>,
}
