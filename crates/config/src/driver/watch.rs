use std::{
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use kernel::{
    Cmd,
    domain::{ConfigName, ThemeName},
    update::{Machine, Unhandled},
};

use crate::{
    driver::{ConfigChange, ConfigMessage, ConfigPaths},
    theme_file_name,
};

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
            keys: WatchedPath::starting(
                paths.config.clone(),
                paths.seen.config.as_deref(),
            ),
            theme: paths.theme.clone().map(|name| SelectedTheme {
                name,
                seen: Seen::Unread,
            }),
            themes: paths.themes.clone(),
            theme_list: Seen::Unread,
        }
    }

    fn theme_path(&self, name: &ThemeName) -> PathBuf {
        self.themes.join(theme_file_name(name.as_str()))
    }
}

#[derive(Debug)]
pub(crate) enum WatchMessage {
    PollAppearance,
    PollConfig,
    PollTheme,
    PollThemes,
    Observed {
        file: ConfigName,
        text: Option<String>,
    },
    Listed(Vec<ThemeName>),
    SelectTheme(ThemeName),
    Wrote {
        file: ConfigName,
        text: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WatchEffect {
    Read { file: ConfigName, path: PathBuf },
    List(PathBuf),
}

impl Machine for ConfigWatch {
    type Message = WatchMessage;
    type Effect = Cmd<WatchEffect, ConfigMessage>;

    fn transition(
        &mut self,
        message: WatchMessage,
    ) -> Result<Cmd<WatchEffect, ConfigMessage>, Unhandled> {
        match message {
            WatchMessage::PollAppearance => {
                Ok(read(ConfigName::Appearance, &self.appearance.path))
            }
            WatchMessage::PollConfig => Ok(read(ConfigName::Config, &self.keys.path)),
            WatchMessage::Wrote {
                file: ConfigName::Theme(_),
                ..
            } => Err(Unhandled),
            WatchMessage::PollTheme => self.poll_theme(),
            WatchMessage::PollThemes => {
                Ok(Cmd::effect(WatchEffect::List(self.themes.clone())))
            }
            WatchMessage::Observed {
                file: ConfigName::Appearance,
                text,
            } => Ok(observed(
                &mut self.appearance.seen,
                text,
                ConfigChange::Appearance,
            )),
            WatchMessage::Observed {
                file: ConfigName::Config,
                text,
            } => Ok(observed(&mut self.keys.seen, text, ConfigChange::Keymap)),
            WatchMessage::Observed {
                file: ConfigName::Theme(_),
                text,
            } => self.theme_observed(text),
            WatchMessage::Listed(names) => Ok(self.themes_listed(names)),
            WatchMessage::SelectTheme(name) => self.select_theme(name),
            WatchMessage::Wrote {
                file: ConfigName::Appearance,
                text,
            } => {
                self.appearance.seen = Seen::of(Some(&text));
                Ok(Cmd::none())
            }
            WatchMessage::Wrote {
                file: ConfigName::Config,
                text,
            } => {
                self.keys.seen = Seen::of(Some(&text));
                Ok(Cmd::none())
            }
        }
    }
}

impl ConfigWatch {
    fn poll_theme(&self) -> Result<Cmd<WatchEffect, ConfigMessage>, Unhandled> {
        let theme = self.theme.as_ref().ok_or(Unhandled)?;
        Ok(read(
            ConfigName::Theme(theme.name.clone()),
            &self.theme_path(&theme.name),
        ))
    }

    fn theme_observed(
        &mut self,
        text: Option<String>,
    ) -> Result<Cmd<WatchEffect, ConfigMessage>, Unhandled> {
        let theme = self.theme.as_mut().ok_or(Unhandled)?;
        let name = theme.name.clone();
        Ok(observed(&mut theme.seen, text, |text| {
            ConfigChange::Theme { name, text }
        }))
    }

    fn select_theme(
        &mut self,
        name: ThemeName,
    ) -> Result<Cmd<WatchEffect, ConfigMessage>, Unhandled> {
        if self.theme.as_ref().is_some_and(|theme| theme.name == name) {
            return Err(Unhandled);
        }
        let reading = read(ConfigName::Theme(name.clone()), &self.theme_path(&name));
        self.theme = Some(SelectedTheme {
            name,
            seen: Seen::Unread,
        });
        Ok(reading)
    }

    fn themes_listed(
        &mut self,
        names: Vec<ThemeName>,
    ) -> Cmd<WatchEffect, ConfigMessage> {
        let listing = names
            .iter()
            .map(ThemeName::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        if !self.theme_list.changed_by(Some(&listing)) {
            return Cmd::none();
        }
        self.theme_list = Seen::of(Some(&listing));
        Cmd::message(ConfigMessage::Changed(ConfigChange::Themes(names)))
    }
}

fn observed(
    seen: &mut Seen,
    text: Option<String>,
    change: impl FnOnce(Option<String>) -> ConfigChange,
) -> Cmd<WatchEffect, ConfigMessage> {
    if !seen.changed_by(text.as_deref()) {
        return Cmd::none();
    }
    *seen = Seen::of(text.as_deref());
    Cmd::message(ConfigMessage::Changed(change(text)))
}

fn read(file: ConfigName, path: &Path) -> Cmd<WatchEffect, ConfigMessage> {
    Cmd::effect(WatchEffect::Read {
        file,
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        Cmd,
        domain::{ConfigName, ThemeName},
        update::{Machine, Unhandled},
    };
    use proptest::{option, prop_assert, prop_assume, proptest};
    use rstest::rstest;

    use crate::driver::{
        ConfigMessage,
        ConfigPaths,
        SeenTexts,
        watch::{ConfigChange, ConfigWatch, Seen, WatchEffect, WatchMessage},
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

    fn reads(file: ConfigName, path: &str) -> Cmd<WatchEffect, ConfigMessage> {
        Cmd::effect(WatchEffect::Read {
            file,
            path: PathBuf::from(path),
        })
    }

    #[rstest]
    #[case::appearance(
        WatchMessage::PollAppearance,
        reads(ConfigName::Appearance, "/config/sifr-ui.toml")
    )]
    #[case::config(
        WatchMessage::PollConfig,
        reads(ConfigName::Config, "/config/config.toml")
    )]
    #[case::theme(WatchMessage::PollTheme, reads(noir(), "/config/themes/noir.toml"))]
    #[case::themes(
        WatchMessage::PollThemes,
        Cmd::effect(WatchEffect::List(PathBuf::from("/config/themes")))
    )]
    #[case::seen_at_start(WatchMessage::Observed { file: ConfigName::Appearance, text: Some("seen".to_string()) }, Cmd::none())]
    #[case::first_sighting(WatchMessage::Observed { file: ConfigName::Config, text: None }, Cmd::message(ConfigMessage::Changed(ConfigChange::Keymap(None))))]
    #[case::own_write(WatchMessage::Wrote { file: ConfigName::Config, text: "x".to_string() }, Cmd::none())]
    #[case::other_theme(
        WatchMessage::SelectTheme(ThemeName::from_static("ink")),
        reads(
            ConfigName::Theme(ThemeName::from_static("ink")),
            "/config/themes/ink.toml"
        )
    )]
    fn a_selected_watch_answers(
        #[case] message: WatchMessage,
        #[case] expected: Cmd<WatchEffect, ConfigMessage>,
    ) {
        let answer = watch(Some("noir")).transition(message).unwrap();

        assert_eq!(answer, expected);
    }

    #[rstest]
    #[case::poll(WatchMessage::PollTheme)]
    #[case::observed(WatchMessage::Observed { file: noir(), text: None })]
    #[case::theme_write(WatchMessage::Wrote { file: noir(), text: String::new() })]
    fn an_unselected_watch_refuses_the_theme(#[case] message: WatchMessage) {
        let mut unselected = watch(None);

        assert_eq!(unselected.transition(message), Err(Unhandled));
        assert_eq!(unselected, watch(None));
    }

    #[test]
    fn selecting_the_current_theme_is_refused() {
        let mut selected = watch(Some("noir"));

        let answer = selected
            .transition(WatchMessage::SelectTheme(ThemeName::from_static("noir")));

        assert_eq!(answer, Err(Unhandled));
    }

    #[test]
    fn an_own_write_is_not_reported_back() {
        let mut watch = watch(None);
        let text = "[window]\n".to_string();
        let written = watch.transition(WatchMessage::Wrote {
            file: ConfigName::Appearance,
            text: text.clone(),
        });
        assert_eq!(written, Ok(Cmd::none()));

        let answer = watch.transition(WatchMessage::Observed {
            file: ConfigName::Appearance,
            text: Some(text),
        });

        assert_eq!(answer, Ok(Cmd::none()));
    }

    #[test]
    fn a_theme_listing_is_reported_once() {
        let mut watch = watch(None);
        let listed = || WatchMessage::Listed(vec![ThemeName::from_static("mine")]);

        let first = watch.transition(listed()).unwrap();
        let second = watch.transition(listed()).unwrap();

        assert_eq!(
            first,
            Cmd::message(ConfigMessage::Changed(ConfigChange::Themes(vec![
                ThemeName::from_static("mine")
            ])))
        );
        assert_eq!(second, Cmd::none());
    }
}
