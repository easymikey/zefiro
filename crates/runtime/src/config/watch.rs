use std::path::{Path, PathBuf};

use config::theme_file_name;
use kernel::{
    IoError,
    domain::{ConfigName, ThemeName},
    update::{Machine, Unhandled},
};

use crate::config::{ConfigPaths, seen::Seen};

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
            theme: paths
                .theme
                .clone()
                .and_then(|name| ThemeName::new(name).ok())
                .map(|name| SelectedTheme {
                    name,
                    seen: Seen::starting(paths.seen.theme.as_deref()),
                }),
            themes: paths.themes.clone(),
            theme_list: Seen::Never,
        }
    }

    fn theme_path(&self, name: &ThemeName) -> PathBuf {
        self.themes.join(theme_file_name(name.as_str()))
    }
}

#[derive(Debug)]
pub(crate) enum WatchMessage {
    Poll(ConfigName),
    PollTheme,
    PollThemes,
    Observed {
        file: ConfigName,
        text: Option<String>,
    },
    Unreadable {
        file: ConfigName,
        kind: IoError,
    },
    ThemesUnreadable(IoError),
    Listed(Vec<String>),
    SelectTheme(String),
    Wrote {
        file: ConfigName,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigChange {
    Appearance(Option<String>),
    Keymap(Option<String>),
    Theme {
        name: ThemeName,
        text: Option<String>,
    },
    Themes(Vec<String>),
    Unreadable {
        file: ConfigName,
        kind: IoError,
    },
    ThemesUnreadable(IoError),
}

#[derive(Debug, Default)]
pub(crate) enum WatchEffect {
    Read {
        file: ConfigName,
        path: PathBuf,
    },
    List(PathBuf),
    Changed(ConfigChange),
    #[default]
    Nothing,
}

impl Machine for ConfigWatch {
    type Message = WatchMessage;
    type Effect = WatchEffect;

    fn transition(&mut self, message: WatchMessage) -> Result<WatchEffect, Unhandled> {
        match message {
            WatchMessage::Poll(ConfigName::Appearance) => {
                Ok(read(ConfigName::Appearance, &self.appearance.path))
            }
            WatchMessage::PollThemes => Ok(WatchEffect::List(self.themes.clone())),
            WatchMessage::Listed(names) => Ok(self.themes_listed(names)),
            WatchMessage::Poll(ConfigName::Config) => {
                Ok(read(ConfigName::Config, &self.keys.path))
            }
            WatchMessage::PollTheme => self.poll_theme(),
            WatchMessage::Poll(ConfigName::Theme(_)) => Err(Unhandled),
            WatchMessage::Observed {
                file: ConfigName::Appearance,
                text,
            } => Ok(self.appearance_observed(text)),
            WatchMessage::Observed {
                file: ConfigName::Config,
                text,
            } => Ok(self.config_observed(text)),
            WatchMessage::Observed {
                file: ConfigName::Theme(_),
                text,
            } => self.theme_observed(text),
            WatchMessage::Unreadable { file, kind } => {
                Ok(WatchEffect::Changed(ConfigChange::Unreadable {
                    file,
                    kind,
                }))
            }
            WatchMessage::ThemesUnreadable(kind) => {
                Ok(WatchEffect::Changed(ConfigChange::ThemesUnreadable(kind)))
            }
            WatchMessage::SelectTheme(name) => self.select_theme(name),
            WatchMessage::Wrote {
                file: ConfigName::Appearance,
                text,
            } => Ok(self.appearance_written(&text)),
            WatchMessage::Wrote {
                file: ConfigName::Config,
                text,
            } => Ok(self.keys_written(&text)),
            WatchMessage::Wrote {
                file: ConfigName::Theme(_),
                ..
            } => Ok(WatchEffect::Nothing),
        }
    }
}

impl ConfigWatch {
    fn config_observed(&mut self, text: Option<String>) -> WatchEffect {
        if !self.keys.seen.changed_by(text.as_deref()) {
            return WatchEffect::Nothing;
        }
        self.keys.seen = Seen::of(text.as_deref());
        WatchEffect::Changed(ConfigChange::Keymap(text))
    }

    fn keys_written(&mut self, text: &str) -> WatchEffect {
        self.keys.seen = Seen::of(Some(text));
        WatchEffect::Nothing
    }

    fn poll_theme(&self) -> Result<WatchEffect, Unhandled> {
        let theme = self.theme.as_ref().ok_or(Unhandled)?;
        Ok(read(
            ConfigName::Theme(theme.name.clone()),
            &self.theme_path(&theme.name),
        ))
    }

    fn appearance_observed(&mut self, text: Option<String>) -> WatchEffect {
        if !self.appearance.seen.changed_by(text.as_deref()) {
            return WatchEffect::Nothing;
        }
        self.appearance.seen = Seen::of(text.as_deref());
        WatchEffect::Changed(ConfigChange::Appearance(text))
    }

    fn theme_observed(
        &mut self,
        text: Option<String>,
    ) -> Result<WatchEffect, Unhandled> {
        let theme = self.theme.as_mut().ok_or(Unhandled)?;
        if !theme.seen.changed_by(text.as_deref()) {
            return Ok(WatchEffect::Nothing);
        }
        theme.seen = Seen::of(text.as_deref());
        let name = theme.name.clone();
        Ok(WatchEffect::Changed(ConfigChange::Theme { name, text }))
    }

    fn select_theme(&mut self, name: String) -> Result<WatchEffect, Unhandled> {
        let name = ThemeName::new(name).map_err(|_| Unhandled)?;
        if self.theme.as_ref().is_some_and(|theme| theme.name == name) {
            return Err(Unhandled);
        }
        let io = read(ConfigName::Theme(name.clone()), &self.theme_path(&name));
        self.theme = Some(SelectedTheme {
            name,
            seen: Seen::Never,
        });
        Ok(io)
    }

    fn themes_listed(&mut self, names: Vec<String>) -> WatchEffect {
        let listing = names.join("\n");
        if !self.theme_list.changed_by(Some(&listing)) {
            return WatchEffect::Nothing;
        }
        self.theme_list = Seen::of(Some(&listing));
        WatchEffect::Changed(ConfigChange::Themes(names))
    }

    fn appearance_written(&mut self, text: &str) -> WatchEffect {
        self.appearance.seen = Seen::of(Some(text));
        WatchEffect::Nothing
    }
}

fn read(file: ConfigName, path: &Path) -> WatchEffect {
    WatchEffect::Read {
        file,
        path: path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    fn noir() -> ConfigName {
        ConfigName::Theme(ThemeName::from_static("noir"))
    }

    use std::path::{Path, PathBuf};

    use kernel::{
        domain::{ConfigName, ThemeName},
        update::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::config::{
        ConfigPaths,
        SeenTexts,
        watch::{ConfigChange, ConfigWatch, WatchEffect, WatchMessage},
    };

    const UI_PATH: &str = "/config/sifr-ui.toml";
    const THEMES_DIR: &str = "/config/themes";
    const KEYS_PATH: &str = "/config/config.toml";

    fn paths(theme: Option<&str>) -> ConfigPaths {
        ConfigPaths {
            config: PathBuf::from(KEYS_PATH),
            appearance: PathBuf::from(UI_PATH),
            themes: PathBuf::from(THEMES_DIR),
            theme: theme.map(str::to_string),
            seen: SeenTexts::default(),
        }
    }

    fn seen_at_start(appearance: &str, theme: &str, keys: &str) -> ConfigWatch {
        let mut start = paths(Some("noir"));
        start.seen = SeenTexts {
            appearance: Some(appearance.to_string()),
            theme: Some(theme.to_string()),
            config: Some(keys.to_string()),
        };
        ConfigWatch::new(&start)
    }

    #[rstest]
    #[case(ConfigName::Appearance, "a = 1\n")]
    #[case(noir(), "b = 2\n")]
    #[case(ConfigName::Config, "c = 3\n")]
    fn a_seen_start_text_is_not_reported_on_the_first_poll(
        #[case] file: ConfigName,
        #[case] text: &str,
    ) {
        let watch = seen_at_start("a = 1\n", "b = 2\n", "c = 3\n");
        let (_, io) = step(watch, observed(file, Some(text)));
        assert!(is_nothing(&io));
    }

    #[rstest]
    #[case(ConfigName::Appearance, "a = 1 \n")]
    #[case(noir(), "b = 3\n")]
    #[case(ConfigName::Config, "c = 3")]
    fn a_changed_file_after_start_is_reported(
        #[case] file: ConfigName,
        #[case] text: &str,
    ) {
        let watch = seen_at_start("a = 1\n", "b = 2\n", "c = 3\n");
        let (_, io) = step(watch, observed(file, Some(text)));
        assert!(sent(io).is_some());
    }

    fn unselected() -> ConfigWatch {
        ConfigWatch::new(&paths(None))
    }

    fn selected(name: &str) -> ConfigWatch {
        ConfigWatch::new(&paths(Some(name)))
    }

    fn observed(file: ConfigName, text: Option<&str>) -> WatchMessage {
        WatchMessage::Observed {
            file,
            text: text.map(str::to_string),
        }
    }

    type Pair = (ConfigWatch, WatchEffect);

    fn step(mut state: ConfigWatch, message: WatchMessage) -> Pair {
        let io = state.transition(message).unwrap_or(WatchEffect::Nothing);
        (state, io)
    }

    fn reads(io: &WatchEffect, file: &ConfigName, path: &str) -> bool {
        match io {
            WatchEffect::Read {
                file: read_file,
                path: read_path,
            } => read_file == file && read_path == Path::new(path),
            WatchEffect::List(_) | WatchEffect::Changed(_) | WatchEffect::Nothing => {
                false
            }
        }
    }

    fn sent(io: WatchEffect) -> Option<ConfigChange> {
        match io {
            WatchEffect::Changed(change) => Some(change),
            WatchEffect::Read { .. } | WatchEffect::List(_) | WatchEffect::Nothing => {
                None
            }
        }
    }

    fn is_nothing(io: &WatchEffect) -> bool {
        matches!(io, WatchEffect::Nothing)
    }

    #[rstest]
    #[case(ConfigName::Appearance, UI_PATH)]
    #[case(ConfigName::Config, KEYS_PATH)]
    fn poll_asks_for_the_file_it_watches(#[case] file: ConfigName, #[case] path: &str) {
        let (_, io) = step(selected("noir"), WatchMessage::Poll(file.clone()));
        assert!(reads(&io, &file, path));
    }

    #[test]
    fn polling_the_theme_asks_for_the_selected_theme_file() {
        let (_, io) = step(selected("noir"), WatchMessage::PollTheme);
        assert!(reads(&io, &noir(), "/config/themes/noir.toml"));
    }

    #[rstest]
    #[case(WatchMessage::PollTheme)]
    #[case(observed(noir(), Some("name = \"noir\"")))]
    fn the_theme_target_refuses_while_unselected(#[case] message: WatchMessage) {
        let mut watch = unselected();
        let reason = watch.transition(message).err();
        assert_eq!(watch, unselected());
        assert_eq!(reason, Some(Unhandled));
    }

    #[rstest]
    #[case(ConfigName::Appearance, Some("[window]\nkey_hints = false\n"))]
    #[case(ConfigName::Appearance, None)]
    #[case(ConfigName::Config, Some("[keymap]\nnext = \"x\"\n"))]
    #[case(ConfigName::Config, None)]
    #[case(noir(), Some("name = \"noir\"\n"))]
    #[case(noir(), None)]
    fn the_first_observation_is_reported(
        #[case] file: ConfigName,
        #[case] text: Option<&str>,
    ) {
        let (_, io) = step(selected("noir"), observed(file, text));
        let change = sent(io).unwrap();
        let reported = match change {
            ConfigChange::Appearance(content) | ConfigChange::Keymap(content) => {
                content
            }
            ConfigChange::Theme {
                name,
                text: content,
            } => {
                assert_eq!("noir", name);
                content
            }
            ConfigChange::Themes(_)
            | ConfigChange::Unreadable { .. }
            | ConfigChange::ThemesUnreadable(_) => None,
        };
        assert_eq!(reported.as_deref(), text);
    }

    #[rstest]
    #[case(ConfigName::Appearance, Some("[window]\nkey_hints = false\n"))]
    #[case(ConfigName::Appearance, None)]
    #[case(ConfigName::Config, Some("[keymap]\nnext = \"x\"\n"))]
    #[case(ConfigName::Config, None)]
    #[case(noir(), Some("name = \"noir\"\n"))]
    #[case(noir(), None)]
    fn an_unchanged_observation_reports_nothing(
        #[case] file: ConfigName,
        #[case] text: Option<&str>,
    ) {
        let (settled, _) = step(selected("noir"), observed(file.clone(), text));
        let (_, io) = step(settled, observed(file, text));
        assert!(is_nothing(&io));
    }

    #[rstest]
    #[case(
        ConfigName::Appearance,
        "[window]\nkey_hints = false\n",
        "[window]\nkey_hints = true\n"
    )]
    #[case(
        ConfigName::Config,
        "[keymap]\nnext = \"x\"\n",
        "[keymap]\nnext = \"y\"\n"
    )]
    fn a_changed_observation_is_reported_again(
        #[case] file: ConfigName,
        #[case] first: &str,
        #[case] second: &str,
    ) {
        let (settled, _) = step(selected("noir"), observed(file.clone(), Some(first)));
        let (_, io) = step(settled, observed(file, Some(second)));
        let content = match sent(io).unwrap() {
            ConfigChange::Appearance(content) | ConfigChange::Keymap(content) => {
                content
            }
            ConfigChange::Theme { .. }
            | ConfigChange::Themes(_)
            | ConfigChange::Unreadable { .. }
            | ConfigChange::ThemesUnreadable(_) => None,
        };
        assert_eq!(content.as_deref(), Some(second));
    }

    #[rstest]
    #[case(ConfigName::Appearance)]
    #[case(ConfigName::Config)]
    fn the_drivers_own_write_is_not_reported_back(#[case] file: ConfigName) {
        let written = "written\n";
        let wrote = WatchMessage::Wrote {
            file: file.clone(),
            text: written.to_string(),
        };
        let (after_write, io) = step(selected("noir"), wrote);
        assert!(is_nothing(&io));
        let (_, io_after_observe) = step(after_write, observed(file, Some(written)));
        assert!(is_nothing(&io_after_observe));
    }

    #[test]
    fn selecting_another_theme_repoints_and_reads_at_once() {
        let (state, io) = step(
            selected("noir"),
            WatchMessage::SelectTheme("wafer".to_string()),
        );
        assert!(reads(
            &io,
            &ConfigName::Theme(ThemeName::from_static("wafer")),
            "/config/themes/wafer.toml",
        ));
        assert_eq!(state, selected("wafer"));
    }

    #[test]
    fn a_reselected_theme_with_no_file_still_reports() {
        let (state, _) = step(
            selected("noir"),
            WatchMessage::SelectTheme("wafer".to_string()),
        );
        let (_, io) = step(state, observed(noir(), None));
        assert_eq!(
            sent(io),
            Some(ConfigChange::Theme {
                name: ThemeName::from_static("wafer"),
                text: None,
            })
        );
    }

    #[test]
    fn selecting_the_current_theme_is_refused() {
        let mut watch = selected("noir");
        let reason = watch
            .transition(WatchMessage::SelectTheme("noir".to_string()))
            .err();
        assert_eq!(watch, selected("noir"));
        assert_eq!(reason, Some(Unhandled));
    }

    #[test]
    fn selecting_a_theme_while_unselected_arms_the_target() {
        let (state, io) =
            step(unselected(), WatchMessage::SelectTheme("noir".to_string()));
        assert!(reads(&io, &noir(), "/config/themes/noir.toml"));
        assert_eq!(state, selected("noir"));
    }

    #[test]
    fn a_keys_file_that_cannot_be_read_is_reported_as_no_text() {
        let (settled, _) = step(
            selected("noir"),
            observed(ConfigName::Config, Some("[keymap]\nnext = \"x\"\n")),
        );
        let (_, io) = step(settled, observed(ConfigName::Config, None));
        assert_eq!(sent(io), Some(ConfigChange::Keymap(None)));
    }

    fn listed(names: &[&str]) -> WatchMessage {
        WatchMessage::Listed(names.iter().map(|name| (*name).to_string()).collect())
    }

    fn themes(io: WatchEffect) -> Option<Vec<String>> {
        match sent(io) {
            Some(ConfigChange::Themes(names)) => Some(names),
            Some(
                ConfigChange::Appearance(_)
                | ConfigChange::Keymap(_)
                | ConfigChange::Theme { .. }
                | ConfigChange::Unreadable { .. }
                | ConfigChange::ThemesUnreadable(_),
            )
            | None => None,
        }
    }

    #[test]
    fn polling_asks_for_the_themes_directory() {
        let (_, io) = step(selected("noir"), WatchMessage::PollThemes);
        assert!(matches!(io, WatchEffect::List(dir) if dir == Path::new(THEMES_DIR)));
    }

    #[test]
    fn a_new_theme_file_is_reported_once() {
        let (settled, first) = step(selected("noir"), listed(&["mine"]));
        let (settled, second) = step(settled, listed(&["mine", "yours"]));
        let (_, again) = step(settled, listed(&["mine", "yours"]));
        assert_eq!(themes(first), Some(vec!["mine".to_string()]));
        assert_eq!(
            themes(second),
            Some(vec!["mine".to_string(), "yours".to_string()])
        );
        assert_eq!(themes(again), None);
    }

    #[test]
    fn a_removed_theme_file_is_reported_once() {
        let (settled, _) = step(selected("noir"), listed(&["mine", "yours"]));
        let (settled, removed) = step(settled, listed(&["mine"]));
        let (_, again) = step(settled, listed(&["mine"]));
        assert_eq!(themes(removed), Some(vec!["mine".to_string()]));
        assert_eq!(themes(again), None);
    }

    #[rstest]
    #[case(kernel::IoError::Denied)]
    fn an_unreadable_file_is_reported_without_being_marked_seen(
        #[case] error: kernel::IoError,
    ) {
        let (state, io) = step(
            selected("noir"),
            WatchMessage::Unreadable {
                file: noir(),
                kind: error,
            },
        );
        assert_eq!(
            sent(io),
            Some(ConfigChange::Unreadable {
                file: noir(),
                kind: error,
            })
        );
        assert_eq!(state, selected("noir"));
    }
}
