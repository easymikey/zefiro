use std::{collections::HashMap, time::Duration};

use crate::domain::{
    ChordPrefix,
    Cursor,
    Keymap,
    Moment,
    Overlay,
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
    pub(crate) source_errors: ConfigFileErrors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigFile {
    Appearance,
    Config,
    Theme,
}

impl std::fmt::Display for ConfigFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ConfigFile::Appearance => "the appearance file",
            ConfigFile::Config => "the config file",
            ConfigFile::Theme => "the theme file",
        };
        formatter.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("{file} is unreadable: {detail}")]
    Unreadable { file: ConfigFile, detail: String },
    #[error("the themes folder is unreadable: {detail}")]
    ThemesUnreadable { detail: String },
    #[error("{file} could not be saved: {detail}")]
    Save { file: ConfigFile, detail: String },
    #[error("Config watch failed: {detail}")]
    Watch { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ConfigFileErrors(HashMap<ConfigFile, String>);

impl ConfigFileErrors {
    pub(crate) fn insert_if_changed(
        &mut self,
        source: ConfigFile,
        text: String,
    ) -> Option<String> {
        if self.0.get(&source) == Some(&text) {
            return None;
        }
        self.0.insert(source, text.clone());
        Some(text)
    }

    pub(crate) fn clear(&mut self, source: ConfigFile) -> Option<String> {
        self.0.remove(&source)
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
    Failure,
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
                phase: SavePhase::Failure,
            })
        );
    }

    #[test]
    fn no_overlay_shows_nothing() {
        assert_eq!(Workspace::default().save_line(), None);
    }
}
