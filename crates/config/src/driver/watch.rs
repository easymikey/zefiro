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
    Keymap(Option<String>),
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
    fn of(text: &str) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Seen {
    #[default]
    Absent,
    Unread,
    Content(Signature),
}

impl Seen {
    fn of(text: Option<&str>) -> Self {
        text.map_or(Seen::Absent, |text| Seen::Content(Signature::of(text)))
    }

    fn starting(text: Option<&str>) -> Self {
        text.map_or(Seen::Unread, |text| Seen::Content(Signature::of(text)))
    }

    fn changed_by(self, text: Option<&str>) -> bool {
        match self {
            Seen::Unread => true,
            Seen::Absent | Seen::Content(_) => self != Seen::of(text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchedPath {
    path: PathBuf,
    seen: Seen,
}

impl WatchedPath {
    fn starting(path: PathBuf, text: Option<&str>) -> Self {
        Self {
            path,
            seen: Seen::starting(text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedTheme {
    name: ThemeName,
    seen: Seen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfigWatch {
    appearance: WatchedPath,
    keys: WatchedPath,
    theme: Option<SelectedTheme>,
    themes: PathBuf,
    theme_list: Seen,
}

impl ConfigWatch {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths) -> Self {
        Self {
            appearance: WatchedPath::starting(
                paths.appearance.clone(),
                paths.seen.appearance.as_deref(),
            ),
            keys: WatchedPath {
                path: paths.config.clone(),
                seen: Seen::of(paths.seen.config.as_deref()),
            },
            theme: paths.theme.clone().map(|name| SelectedTheme {
                name,
                seen: Seen::Unread,
            }),
            themes: paths.themes.clone(),
            theme_list: Seen::Unread,
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
        name: ConfigName,
        text: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum WatchEffect {
    Read { file: ConfigName, path: PathBuf },
    List(PathBuf),
}

impl Machine for ConfigWatch {
    type Message = ConfigWatchMessage;
    type Effect = Cmd<WatchEffect, ConfigChange>;

    fn transition(
        &mut self,
        message: ConfigWatchMessage,
    ) -> Result<Cmd<WatchEffect, ConfigChange>, Unhandled> {
        match message {
            ConfigWatchMessage::PollAppearance => {
                Ok(read(ConfigName::Appearance, &self.appearance.path))
            }
            ConfigWatchMessage::PollConfig => {
                Ok(read(ConfigName::Config, &self.keys.path))
            }
            ConfigWatchMessage::Saved {
                name: ConfigName::Theme(_),
                ..
            } => Err(Unhandled),
            ConfigWatchMessage::PollTheme => self.poll_theme(),
            ConfigWatchMessage::PollThemes => {
                Ok(Cmd::effect(WatchEffect::List(self.themes.clone())))
            }
            ConfigWatchMessage::ReadDone {
                name: ConfigName::Appearance,
                text,
            } => Ok(observed(
                &mut self.appearance.seen,
                text,
                ConfigChange::Appearance,
            )),
            ConfigWatchMessage::ReadDone {
                name: ConfigName::Config,
                text,
            } => Ok(observed(&mut self.keys.seen, text, ConfigChange::Keymap)),
            ConfigWatchMessage::ReadDone {
                name: ConfigName::Theme(_),
                text,
            } => self.theme_observed(text),
            ConfigWatchMessage::Listed {
                theme_names,
                refused,
            } => Ok(self.themes_listed(theme_names, refused)),
            ConfigWatchMessage::SelectTheme(name) => self.select_theme(name),
            ConfigWatchMessage::Saved {
                name: ConfigName::Appearance,
                text,
            } => {
                self.appearance.seen = Seen::of(Some(&text));
                Ok(Cmd::none())
            }
            ConfigWatchMessage::Saved {
                name: ConfigName::Config,
                text,
            } => {
                self.keys.seen = Seen::of(Some(&text));
                Ok(Cmd::none())
            }
        }
    }
}

impl ConfigWatch {
    fn poll_theme(&self) -> Result<Cmd<WatchEffect, ConfigChange>, Unhandled> {
        let name = self.theme.as_ref().ok_or(Unhandled)?.name.clone();
        let path = theme_file_path(&self.themes, &name);
        Ok(read(ConfigName::Theme(name), &path))
    }

    fn theme_observed(
        &mut self,
        text: Option<String>,
    ) -> Result<Cmd<WatchEffect, ConfigChange>, Unhandled> {
        let theme = self.theme.as_mut().ok_or(Unhandled)?;
        let name = theme.name.clone();
        Ok(observed(&mut theme.seen, text, |text| {
            ConfigChange::Theme { name, text }
        }))
    }

    fn select_theme(
        &mut self,
        name: ThemeName,
    ) -> Result<Cmd<WatchEffect, ConfigChange>, Unhandled> {
        if self.theme.as_ref().is_some_and(|theme| theme.name == name) {
            return Err(Unhandled);
        }
        self.theme = Some(SelectedTheme {
            name,
            seen: Seen::Unread,
        });
        self.poll_theme()
    }

    fn themes_listed(
        &mut self,
        theme_names: Vec<ThemeName>,
        refused: Vec<String>,
    ) -> Cmd<WatchEffect, ConfigChange> {
        let listing = theme_names
            .iter()
            .map(ThemeName::as_str)
            .chain(std::iter::once("\0"))
            .chain(refused.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        if !self.theme_list.changed_by(Some(&listing)) {
            return Cmd::none();
        }
        self.theme_list = Seen::of(Some(&listing));
        Cmd::message(ConfigChange::Themes {
            theme_names,
            refused,
        })
    }
}

fn observed(
    seen: &mut Seen,
    text: Option<String>,
    change: impl FnOnce(Option<String>) -> ConfigChange,
) -> Cmd<WatchEffect, ConfigChange> {
    if !seen.changed_by(text.as_deref()) {
        return Cmd::none();
    }
    *seen = Seen::of(text.as_deref());
    Cmd::message(change(text))
}

fn read(file: ConfigName, path: &Path) -> Cmd<WatchEffect, ConfigChange> {
    Cmd::effect(WatchEffect::Read {
        file,
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
        watch::{ConfigChange, ConfigWatch, ConfigWatchMessage, Seen, WatchEffect},
    };

    proptest! {
        #[test]
        fn a_fresh_target_always_reports_changed(text in option::of(".*")) {
            prop_assert!(Seen::Unread.changed_by(text.as_deref()));
        }

        #[test]
        fn its_own_text_never_reports_changed(text in option::of(".*")) {
            let seen = Seen::of(text.as_deref());
            prop_assert!(!seen.changed_by(text.as_deref()));
        }

        #[test]
        fn a_different_text_reports_changed(
            first in option::of(".*"),
            second in option::of(".*"),
        ) {
            prop_assume!(first != second);
            let seen = Seen::of(first.as_deref());
            prop_assert!(seen.changed_by(second.as_deref()));
        }
    }

    fn watch(theme: Option<&'static str>) -> ConfigWatch {
        ConfigWatch::new(&ConfigPaths {
            config: PathBuf::from("/config/config.toml"),
            appearance: PathBuf::from("/config/sifr-ui.toml"),
            themes: PathBuf::from("/config/themes"),
            default_music_dir: None,
            theme: theme.map(ThemeName::from_static),
            seen: SeenTexts {
                appearance: Some("seen".to_string()),
                config: None,
            },
        })
    }

    fn noir() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("noir"))
    }

    fn reads(file: ConfigName, path: &str) -> Cmd<WatchEffect, ConfigChange> {
        Cmd::effect(WatchEffect::Read {
            file,
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
        Cmd::effect(WatchEffect::List(PathBuf::from("/config/themes")))
    )]
    #[case::seen_at_start(ConfigWatchMessage::ReadDone { name: ConfigName::Appearance, text: Some("seen".to_string()) }, Cmd::none())]
    #[case::absent_config_at_start(ConfigWatchMessage::ReadDone { name: ConfigName::Config, text: None }, Cmd::none())]
    #[case::config_appears(ConfigWatchMessage::ReadDone { name: ConfigName::Config, text: Some("x".to_string()) }, Cmd::message(ConfigChange::Keymap(Some("x".to_string()))))]
    #[case::own_write(ConfigWatchMessage::Saved { name: ConfigName::Config, text: "x".to_string() }, Cmd::none())]
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
        #[case] expected: Cmd<WatchEffect, ConfigChange>,
    ) {
        let answer = watch(Some("noir")).transition(message).unwrap();

        assert_eq!(answer, expected);
    }

    #[rstest]
    #[case::poll(ConfigWatchMessage::PollTheme)]
    #[case::observed(ConfigWatchMessage::ReadDone { name: noir(), text: None })]
    #[case::theme_write(ConfigWatchMessage::Saved { name: noir(), text: String::new() })]
    fn an_unselected_watch_refuses_the_theme(#[case] message: ConfigWatchMessage) {
        let mut unselected = watch(None);

        assert_eq!(unselected.transition(message), Err(Unhandled));
        assert_eq!(unselected, watch(None));
    }

    #[test]
    fn an_absent_theme_is_reported_once() {
        let mut absent_theme = watch(Some("noir"));
        let absent = Cmd::message(ConfigChange::Theme {
            name: ThemeName::from_static("noir"),
            text: None,
        });
        let done = || ConfigWatchMessage::ReadDone {
            name: noir(),
            text: None,
        };

        let first = absent_theme.transition(done());
        let second = absent_theme.transition(done());

        assert_eq!(first, Ok(absent));
        assert_eq!(second, Ok(Cmd::none()));
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
            name: ConfigName::Appearance,
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

    #[test]
    fn a_read_with_unchanged_text_reports_no_change() {
        let mut watch = watch(Some("noir"));
        let read = || ConfigWatchMessage::ReadDone {
            name: noir(),
            text: Some("same".to_string()),
        };

        let first = watch.transition(read()).unwrap();
        let second = watch.transition(read()).unwrap();

        assert_eq!(
            first,
            Cmd::message(ConfigChange::Theme {
                name: ThemeName::from_static("noir"),
                text: Some("same".to_string())
            })
        );
        assert_eq!(second, Cmd::none());
    }
}
