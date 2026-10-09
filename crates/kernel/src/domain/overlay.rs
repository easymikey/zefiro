use std::{fmt, path::PathBuf, sync::Arc};

use strum::{EnumDiscriminants, EnumIter, IntoStaticStr};

use crate::domain::{
    cursor_over::CursorOver,
    index::ViewIndex,
    io_error::IoError,
    playlist::PlaylistFileNameError,
    revision::Revision,
    server::{
        Account,
        EndpointError,
        SecretError,
        ServerName,
        ServerStatus,
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
    ServerSearch,
    SavePlaylist(TextEntry<PlaylistFileNameError>),
    History(CursorOver<()>),
    Settings(SettingRow),
    ConfirmTrash(Arc<Track>),
    JumpToTime(TextEntry<TimecodeError>),
    TrackDetails(Arc<Track>),
    MusicDir {
        text_entry: TextEntry<MusicDirError>,
        verdict: Option<Verdict>,
        revision: Option<Revision>,
        folders: CursorOver<Folders>,
    },
    AddServer(ServerPrompt),
    Servers(CursorOver<()>),
    ConfirmRemove(ServerName),
}

impl Overlay {
    #[must_use]
    pub(crate) fn captures_text(&self) -> bool {
        match self {
            Overlay::Search(_)
            | Overlay::ServerSearch
            | Overlay::SavePlaylist(_)
            | Overlay::MusicDir { .. }
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
    #[error("checking the folder")]
    Pending,
}

impl TextEntry<MusicDirError> {
    #[must_use]
    pub fn path(&self) -> PathBuf {
        let trimmed = self.input.trim();
        let path = trimmed.trim_end_matches('/');
        if path.is_empty() && !trimmed.is_empty() {
            PathBuf::from("/")
        } else {
            PathBuf::from(path)
        }
    }

    #[must_use]
    pub fn folder(&self) -> Option<(PathBuf, &str)> {
        let trimmed = self.input.trim();
        let index = trimmed.rfind('/')?;
        let (folder, segment) = trimmed.split_at(index);
        let folder = if folder.is_empty() { "/" } else { folder };
        Some((PathBuf::from(folder), segment.strip_prefix('/')?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subfolder {
    Audio(String),
    Plain(String),
}

impl Subfolder {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Subfolder::Audio(name) | Subfolder::Plain(name) => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subfolders {
    pub path: PathBuf,
    pub verdict: Verdict,
    pub subfolders: Vec<Subfolder>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folders {
    pub path: PathBuf,
    pub subfolders: Option<Subfolders>,
    pub matches: Vec<usize>,
    pub revision: Option<Revision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Readable,
    Missing,
    NotADirectory,
    Denied,
    Unreadable(IoError),
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Readable => f.write_str("a readable folder"),
            Verdict::Missing => f.write_str("no such path"),
            Verdict::NotADirectory => f.write_str("not a folder"),
            Verdict::Denied => f.write_str(
                "no permission: allow the terminal in Privacy & Security, Files and Folders",
            ),
            Verdict::Unreadable(error) => write!(f, "cannot be read: {error}"),
        }
    }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    #[default]
    Link,
    User,
    Password,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ServerPrompt {
    pub origin_server_name: Option<ServerName>,
    pub link_text_entry: TextEntry<EndpointError>,
    pub user_text_entry: TextEntry<UserNameError>,
    pub password_text_entry: TextEntry<SecretError>,
    pub field: Field,
    pub reached_field: Field,
    pub server_status: Option<ServerStatus>,
}

impl fmt::Debug for ServerPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            origin_server_name,
            link_text_entry,
            user_text_entry,
            password_text_entry,
            field,
            reached_field,
            server_status,
        } = self;
        formatter
            .debug_struct("ServerPrompt")
            .field("origin_server_name", origin_server_name)
            .field("link_text_entry", link_text_entry)
            .field("user_text_entry", user_text_entry)
            .field("password_error", &password_text_entry.error)
            .field("field", field)
            .field("reached_field", reached_field)
            .field("server_status", server_status)
            .finish_non_exhaustive()
    }
}

impl From<&Account> for ServerPrompt {
    fn from(account: &Account) -> Self {
        Self {
            origin_server_name: Some(account.server_name.clone()),
            link_text_entry: TextEntry {
                input: account.endpoint.as_str().to_owned(),
                error: None,
            },
            user_text_entry: TextEntry {
                input: account.user_name.as_str().to_owned(),
                error: None,
            },
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub input: String,
    pub matches: Vec<ViewIndex>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerQuery {
    pub input: String,
    pub catalog_rows: Vec<CatalogRow>,
    pub revision: Option<Revision>,
}
