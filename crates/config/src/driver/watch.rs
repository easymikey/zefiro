use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use kernel::{
    cmd::Cmd,
    domain::{config::ConfigName, theme::ThemeName},
    update::machine::{Machine, Unhandled},
};

use crate::{driver::paths::ConfigPaths, file_name::theme_file_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigChange {
    Appearance(Option<String>),
    Config(Option<String>),
    Theme {
        name: ThemeName,
        text: Option<String>,
    },
    Themes {
        theme_names: Vec<ThemeName>,
        refused: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Signature(u64);

impl Signature {
    fn of(hashed: &impl Hash) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        hashed.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Seen {
    #[default]
    Absent,
    Unread,
    Present(Signature),
}

impl Seen {
    fn from_text(text: Option<&str>) -> Self {
        text.map_or(Seen::Absent, |text| Seen::Present(Signature::of(&text)))
    }

    fn from_start_text(text: Option<&str>) -> Self {
        text.map_or(Seen::Unread, |text| Seen::Present(Signature::of(&text)))
    }

    fn is_changed_to(self, next: Seen) -> bool {
        match self {
            Seen::Unread => true,
            Seen::Absent | Seen::Present(_) => self != next,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchedPath {
    path: PathBuf,
    seen: Seen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchedTheme {
    name: ThemeName,
    seen: Seen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigWatch {
    appearance_path: WatchedPath,
    config_path: WatchedPath,
    watched_theme: Option<WatchedTheme>,
    themes_dir: PathBuf,
    seen: Seen,
}

impl ConfigWatch {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths) -> Self {
        Self {
            appearance_path: WatchedPath {
                path: paths.appearance_path.clone(),
                seen: Seen::from_start_text(paths.seen_texts.appearance.as_deref()),
            },
            config_path: WatchedPath {
                path: paths.config_path.clone(),
                seen: Seen::from_text(paths.seen_texts.config.as_deref()),
            },
            watched_theme: paths.theme_name.clone().map(|name| WatchedTheme {
                name,
                seen: Seen::Unread,
            }),
            themes_dir: paths.themes_dir.clone(),
            seen: Seen::Unread,
        }
    }

    pub(crate) fn path(&self, saved_file: SavedFile) -> &Path {
        match saved_file {
            SavedFile::Config => &self.config_path.path,
            SavedFile::Appearance => &self.appearance_path.path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedFile {
    Config,
    Appearance,
}

impl From<SavedFile> for ConfigName {
    fn from(saved_file: SavedFile) -> Self {
        match saved_file {
            SavedFile::Config => ConfigName::Config,
            SavedFile::Appearance => ConfigName::Appearance,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ConfigWatchMessage {
    PollAppearance,
    PollConfig,
    PollTheme,
    PollThemes,
    ReadDone {
        name: ConfigName,
        text: Option<String>,
    },
    Listed {
        theme_names: Vec<ThemeName>,
        refused: Vec<String>,
    },
    SelectTheme(ThemeName),
    Saved {
        saved_file: SavedFile,
        text: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigWatchEffect {
    Read { name: ConfigName, path: PathBuf },
    List(PathBuf),
}

impl Machine for ConfigWatch {
    type Message = ConfigWatchMessage;
    type Effect = Cmd<ConfigWatchEffect, ConfigChange>;

    fn transition(
        &mut self,
        message: ConfigWatchMessage,
    ) -> Result<Cmd<ConfigWatchEffect, ConfigChange>, Unhandled> {
        match message {
            ConfigWatchMessage::PollAppearance => {
                Ok(read(ConfigName::Appearance, &self.appearance_path.path))
            }
            ConfigWatchMessage::PollConfig => {
                Ok(read(ConfigName::Config, &self.config_path.path))
            }
            ConfigWatchMessage::PollTheme => self.poll_theme(),
            ConfigWatchMessage::PollThemes => Ok(Cmd::effect(ConfigWatchEffect::List(
                self.themes_dir.clone(),
            ))),
            ConfigWatchMessage::ReadDone {
                name: ConfigName::Appearance,
                text,
            } => Ok(read_done(
                &mut self.appearance_path.seen,
                text,
                ConfigChange::Appearance,
            )),
            ConfigWatchMessage::ReadDone {
                name: ConfigName::Config,
                text,
            } => Ok(read_done(
                &mut self.config_path.seen,
                text,
                ConfigChange::Config,
            )),
            ConfigWatchMessage::ReadDone {
                name: ConfigName::Theme(name),
                text,
            } => self.theme_read_done(name, text),
            ConfigWatchMessage::Listed {
                theme_names,
                refused,
            } => Ok(self.themes_listed(theme_names, refused)),
            ConfigWatchMessage::SelectTheme(name) => self.select_theme(name),
            ConfigWatchMessage::Saved {
                saved_file: SavedFile::Appearance,
                text,
            } => {
                self.appearance_path.seen = Seen::from_text(Some(&text));
                Ok(Cmd::none())
            }
            ConfigWatchMessage::Saved {
                saved_file: SavedFile::Config,
                text,
            } => {
                self.config_path.seen = Seen::from_text(Some(&text));
                Ok(Cmd::none())
            }
        }
    }
}

impl ConfigWatch {
    fn poll_theme(&self) -> Result<Cmd<ConfigWatchEffect, ConfigChange>, Unhandled> {
        let name = self.watched_theme.as_ref().ok_or(Unhandled)?.name.clone();
        let path = theme_file_path(&self.themes_dir, &name);
        Ok(read(ConfigName::Theme(name), &path))
    }

    fn theme_read_done(
        &mut self,
        name: ThemeName,
        text: Option<String>,
    ) -> Result<Cmd<ConfigWatchEffect, ConfigChange>, Unhandled> {
        let theme = self
            .watched_theme
            .as_mut()
            .filter(|theme| theme.name == name)
            .ok_or(Unhandled)?;
        Ok(read_done(&mut theme.seen, text, |text| {
            ConfigChange::Theme { name, text }
        }))
    }

    fn select_theme(
        &mut self,
        name: ThemeName,
    ) -> Result<Cmd<ConfigWatchEffect, ConfigChange>, Unhandled> {
        if self
            .watched_theme
            .as_ref()
            .is_some_and(|theme| theme.name == name)
        {
            return Err(Unhandled);
        }
        self.watched_theme = Some(WatchedTheme {
            name,
            seen: Seen::Unread,
        });
        self.poll_theme()
    }

    fn themes_listed(
        &mut self,
        theme_names: Vec<ThemeName>,
        refused: Vec<String>,
    ) -> Cmd<ConfigWatchEffect, ConfigChange> {
        let next = Seen::Present(Signature::of(&(&theme_names, &refused)));
        if !self.seen.is_changed_to(next) {
            return Cmd::none();
        }
        self.seen = next;
        Cmd::message(ConfigChange::Themes {
            theme_names,
            refused,
        })
    }
}

fn read_done(
    seen: &mut Seen,
    text: Option<String>,
    change: impl FnOnce(Option<String>) -> ConfigChange,
) -> Cmd<ConfigWatchEffect, ConfigChange> {
    let next = Seen::from_text(text.as_deref());
    if !seen.is_changed_to(next) {
        return Cmd::none();
    }
    *seen = next;
    Cmd::message(change(text))
}

fn read(config_name: ConfigName, path: &Path) -> Cmd<ConfigWatchEffect, ConfigChange> {
    Cmd::effect(ConfigWatchEffect::Read {
        name: config_name,
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        cmd::Cmd,
        domain::{config::ConfigName, theme::ThemeName},
        update::machine::{Machine, Unhandled},
    };
    use proptest::{option, prop_assert, prop_assume, proptest};
    use rstest::rstest;

    use crate::driver::{
        paths::{ConfigPaths, SeenTexts},
        watch::{
            ConfigChange,
            ConfigWatch,
            ConfigWatchEffect,
            ConfigWatchMessage,
            SavedFile,
            Seen,
        },
    };

    proptest! {
        #[test]
        fn a_different_text_reports_changed(
            first in option::of(".*"),
            second in option::of(".*"),
        ) {
            prop_assume!(first != second);
            let seen = Seen::from_text(first.as_deref());
            prop_assert!(seen.is_changed_to(Seen::from_text(second.as_deref())));
        }
    }

    fn watch(theme: Option<&'static str>) -> ConfigWatch {
        ConfigWatch::new(&ConfigPaths {
            config_path: PathBuf::from("/config/config.toml"),
            appearance_path: PathBuf::from("/config/sifr-ui.toml"),
            themes_dir: PathBuf::from("/config/themes"),
            default_music_dir: None,
            theme_name: theme.map(ThemeName::from_static),
            seen_texts: SeenTexts {
                appearance: Some("seen".to_string()),
                config: None,
            },
        })
    }

    fn noir() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("noir"))
    }

    fn reads(
        config_name: ConfigName,
        path: &str,
    ) -> Cmd<ConfigWatchEffect, ConfigChange> {
        Cmd::effect(ConfigWatchEffect::Read {
            name: config_name,
            path: PathBuf::from(path),
        })
    }

    #[rstest]
    #[case::appearance(
        ConfigWatchMessage::PollAppearance,
        reads(ConfigName::Appearance, "/config/sifr-ui.toml")
    )]
    #[case::config(
        ConfigWatchMessage::PollConfig,
        reads(ConfigName::Config, "/config/config.toml")
    )]
    #[case::theme(
        ConfigWatchMessage::PollTheme,
        reads(noir(), "/config/themes/noir.toml")
    )]
    #[case::themes(
        ConfigWatchMessage::PollThemes,
        Cmd::effect(ConfigWatchEffect::List(PathBuf::from("/config/themes")))
    )]
    #[case::seen_at_start(ConfigWatchMessage::ReadDone { name: ConfigName::Appearance, text: Some("seen".to_string()) }, Cmd::none())]
    #[case::absent_config_at_start(ConfigWatchMessage::ReadDone { name: ConfigName::Config, text: None }, Cmd::none())]
    #[case::config_appears(ConfigWatchMessage::ReadDone { name: ConfigName::Config, text: Some("x".to_string()) }, Cmd::message(ConfigChange::Config(Some("x".to_string()))))]
    #[case::own_write(ConfigWatchMessage::Saved { saved_file: SavedFile::Config, text: "x".to_string() }, Cmd::none())]
    #[case::absent_theme(
        ConfigWatchMessage::ReadDone { name: noir(), text: None },
        Cmd::message(ConfigChange::Theme {
            name: ThemeName::from_static("noir"),
            text: None,
        })
    )]
    #[case::other_theme(
        ConfigWatchMessage::SelectTheme(ThemeName::from_static("ink")),
        reads(
            ConfigName::Theme(ThemeName::from_static("ink")),
            "/config/themes/ink.toml"
        )
    )]
    fn a_selected_watch_answers(
        #[case] message: ConfigWatchMessage,
        #[case] expected: Cmd<ConfigWatchEffect, ConfigChange>,
    ) {
        let answer = watch(Some("noir")).transition(message).unwrap();

        assert_eq!(answer, expected);
    }

    #[rstest]
    #[case::poll(ConfigWatchMessage::PollTheme)]
    #[case::read_done(ConfigWatchMessage::ReadDone { name: noir(), text: None })]
    fn an_unselected_watch_refuses_the_theme(#[case] message: ConfigWatchMessage) {
        let mut unselected = watch(None);

        assert_eq!(unselected.transition(message), Err(Unhandled));
        assert_eq!(unselected, watch(None));
    }

    #[test]
    fn a_read_of_a_theme_no_longer_watched_is_refused() {
        let mut selected = watch(Some("noir"));

        let answer = selected.transition(ConfigWatchMessage::ReadDone {
            name: ConfigName::Theme(ThemeName::from_static("ink")),
            text: Some("ink".to_string()),
        });

        assert_eq!(answer, Err(Unhandled));
        assert_eq!(selected, watch(Some("noir")));
    }

    #[test]
    fn selecting_the_current_theme_is_refused() {
        let mut selected = watch(Some("noir"));

        let answer = selected.transition(ConfigWatchMessage::SelectTheme(
            ThemeName::from_static("noir"),
        ));

        assert_eq!(answer, Err(Unhandled));
    }

    #[test]
    fn an_own_write_is_not_reported_back() {
        let mut watch = watch(None);
        let text = "[window]\n".to_string();
        let written = watch.transition(ConfigWatchMessage::Saved {
            saved_file: SavedFile::Appearance,
            text: text.clone(),
        });
        assert_eq!(written, Ok(Cmd::none()));

        let answer = watch.transition(ConfigWatchMessage::ReadDone {
            name: ConfigName::Appearance,
            text: Some(text),
        });

        assert_eq!(answer, Ok(Cmd::none()));
    }

    #[test]
    fn a_theme_listing_is_reported_once() {
        let mut watch = watch(None);
        let listed = || ConfigWatchMessage::Listed {
            theme_names: vec![ThemeName::from_static("mine")],
            refused: Vec::new(),
        };

        let first = watch.transition(listed()).unwrap();
        let second = watch.transition(listed()).unwrap();

        assert_eq!(
            first,
            Cmd::message(ConfigChange::Themes {
                theme_names: vec![ThemeName::from_static("mine")],
                refused: Vec::new()
            })
        );
        assert_eq!(second, Cmd::none());
    }
}
