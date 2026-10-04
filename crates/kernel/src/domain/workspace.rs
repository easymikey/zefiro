use std::{collections::HashMap, time::Duration};

use crate::domain::{
    ChordPrefix,
    Cursor,
    Keymap,
    Moment,
    Overlay,
    ThemeName,
    ViewIndex,
    library::SortKey,
};

pub const TOAST_SECONDS: u64 = 5;
pub const TOAST_LIFETIME: Duration = Duration::from_secs(TOAST_SECONDS);
pub const TOAST_STACK: usize = 3;

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub overlay: Option<Overlay>,
    pub browse: Browse,
    pub chord_prefix: Option<ChordPrefix>,
    pub toasts: Vec<Toast>,
    pub clock: Moment,
    pub keymap: Keymap,
    pub visible_rows: usize,
    pub played_for: Duration,
    pub(crate) config_errors: ConfigErrors,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConfigName {
    Config,
    Appearance,
    Theme(ThemeName),
}

impl std::fmt::Display for ConfigName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ConfigName::Appearance => "the appearance file",
            ConfigName::Config => "the config file",
            ConfigName::Theme(name) => return write!(formatter, "the theme {name}"),
        };
        formatter.write_str(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IoError {
    #[error("not found")]
    Missing,
    #[error("permission denied")]
    Denied,
    #[error("corrupt data")]
    Malformed,
    #[error("disk full")]
    Full,
    #[error("an unknown error")]
    Other,
}

impl From<std::io::ErrorKind> for IoError {
    fn from(kind: std::io::ErrorKind) -> Self {
        [
            (std::io::ErrorKind::NotFound, IoError::Missing),
            (std::io::ErrorKind::PermissionDenied, IoError::Denied),
            (std::io::ErrorKind::StorageFull, IoError::Full),
            (std::io::ErrorKind::InvalidData, IoError::Malformed),
            (std::io::ErrorKind::UnexpectedEof, IoError::Malformed),
        ]
        .into_iter()
        .find(|(known, _)| *known == kind)
        .map_or(IoError::Other, |(_, error)| error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{file} is unreadable: {kind}")]
    Unreadable { file: ConfigName, kind: IoError },
    #[error("the themes folder is unreadable: {0}")]
    ThemesUnreadable(IoError),
    #[error("{file} could not be saved: {kind}")]
    Save { file: ConfigName, kind: IoError },
    #[error("Config watch failed: {0}")]
    Watch(IoError),
    #[error("{0}")]
    Invalid(Diagnostic),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diagnostic(String);

impl Diagnostic {
    #[must_use]
    pub fn from_error(error: &impl std::error::Error) -> Self {
        Self(error.to_string())
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ConfigErrors(HashMap<ConfigName, ConfigError>);

impl ConfigErrors {
    pub(crate) fn insert_if_changed(
        &mut self,
        name: ConfigName,
        error: ConfigError,
    ) -> bool {
        let unchanged = self.0.get(&name) == Some(&error);
        self.0.insert(name, error);
        !unchanged
    }

    pub(crate) fn clear(&mut self, name: &ConfigName) -> Option<ConfigError> {
        self.0.remove(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Browse {
    pub cursor: Cursor,
    pub sort: SortKey,
}

impl Browse {
    #[must_use]
    pub fn selected(&self) -> ViewIndex {
        ViewIndex::new(self.cursor.index())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    pub title: String,
    pub text: Option<String>,
    pub raised_at: Moment,
}

impl Toast {
    fn of(kind: ToastKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            text: None,
            raised_at: Moment::default(),
        }
    }

    #[must_use]
    pub fn info(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Info, title)
    }

    #[must_use]
    pub fn success(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Success, title)
    }

    #[must_use]
    pub fn warning(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Warning, title)
    }

    #[must_use]
    pub fn error(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Error, title)
    }

    #[must_use]
    pub fn with_text(self, text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePhase {
    Prompt,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveLine {
    pub text: String,
    pub phase: SavePhase,
}

impl Workspace {
    #[must_use]
    pub fn save_line(&self) -> Option<SaveLine> {
        self.overlay.as_ref().and_then(Overlay::save_line)
    }
}

#[cfg(test)]
mod save_line_tests {
    use crate::domain::{
        Overlay,
        TextEntry,
        playlist::PlaylistNameError,
        workspace::{SaveLine, SavePhase, Toast, Workspace},
    };

    #[test]
    fn the_save_playlist_overlay_is_the_only_source_of_a_prompt() {
        let mut workspace = Workspace {
            overlay: Some(Overlay::SavePlaylist {
                typed: TextEntry {
                    input: "mixtape".into(),
                },
                error: None,
            }),
            toasts: vec![Toast::error("stale error")],
            ..Workspace::default()
        };
        assert_eq!(
            workspace.save_line(),
            Some(SaveLine {
                text: "Save playlist: mixtape".to_string(),
                phase: SavePhase::Prompt,
            })
        );

        workspace.overlay = None;
        assert_eq!(workspace.save_line(), None);
    }

    #[test]
    fn a_rejected_name_shows_the_failure_phase() {
        let workspace = Workspace {
            overlay: Some(Overlay::SavePlaylist {
                typed: TextEntry {
                    input: "...".into(),
                },
                error: Some(PlaylistNameError::AllDots),
            }),
            ..Workspace::default()
        };
        assert_eq!(
            workspace.save_line(),
            Some(SaveLine {
                text: PlaylistNameError::AllDots.to_string(),
                phase: SavePhase::Failed,
            })
        );
    }

    #[test]
    fn no_overlay_shows_nothing() {
        assert_eq!(Workspace::default().save_line(), None);
    }
}
