use std::{collections::HashMap, time::Duration};

use crate::domain::{
    ChordPrefix,
    Cursor,
    Keymap,
    Overlay,
    PlaylistIndex,
    library::SortKey,
};

pub const TOAST_LIFETIME: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub overlay: Option<Overlay>,
    pub browse: Browse,
    pub chord: Option<ChordPrefix>,
    pub toast: Option<Toast>,
    pub keymap: Keymap,
    pub played_for: Duration,
    pub(crate) source_errors: SourceErrors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigSource {
    Keymap,
    Theme,
    Appearance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigFile {
    Appearance,
    Keymap,
    Theme,
    ThemeDirectory,
}

impl std::fmt::Display for ConfigFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ConfigFile::Appearance => "the appearance file",
            ConfigFile::Keymap => "the config file",
            ConfigFile::Theme => "the theme file",
            ConfigFile::ThemeDirectory => "the themes folder",
        };
        formatter.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigFailure {
    #[error("{file} is unreadable: {detail}")]
    Unreadable { file: ConfigFile, detail: String },
    #[error("{file} could not be saved: {detail}")]
    Save { file: ConfigFile, detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SourceErrors(HashMap<ConfigSource, String>);

impl SourceErrors {
    pub(crate) fn note(
        &mut self,
        source: ConfigSource,
        text: String,
    ) -> Option<String> {
        if self.0.get(&source) == Some(&text) {
            return None;
        }
        self.0.insert(source, text.clone());
        Some(text)
    }

    pub(crate) fn clear(&mut self, source: ConfigSource) -> Option<String> {
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
    pub fn selected(&self) -> PlaylistIndex {
        PlaylistIndex::new(self.cursor.index())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
}

impl Toast {
    #[must_use]
    pub fn error(text: String) -> Self {
        Self {
            level: ToastLevel::Error,
            text,
        }
    }

    #[must_use]
    pub fn info(text: String) -> Self {
        Self {
            level: ToastLevel::Info,
            text,
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
        match &self.overlay {
            Some(Overlay::SavePlaylist { typed, error: None }) => Some(SaveLine {
                text: format!("Save playlist: {}", typed.input),
                phase: SavePhase::Prompt,
            }),
            Some(Overlay::SavePlaylist {
                error: Some(reason),
                ..
            }) => Some(SaveLine {
                text: reason.to_string(),
                phase: SavePhase::Failure,
            }),
            Some(_) | None => None,
        }
    }
}

#[cfg(test)]
mod save_line_tests {
    use crate::domain::{
        Overlay,
        TextEntry,
        playlist::PlaylistNameRejection,
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
            toast: Some(Toast::error("stale error".into())),
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
                error: Some(PlaylistNameRejection::AllDots),
            }),
            ..Workspace::default()
        };
        assert_eq!(
            workspace.save_line(),
            Some(SaveLine {
                text: PlaylistNameRejection::AllDots.to_string(),
                phase: SavePhase::Failure,
            })
        );
    }

    #[test]
    fn no_overlay_shows_nothing() {
        assert_eq!(Workspace::default().save_line(), None);
    }
}
