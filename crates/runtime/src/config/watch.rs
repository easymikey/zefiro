use std::path::{Path, PathBuf};

use config::theme_file_name;
use kernel::{
    domain::ConfigFile,
    update::{Machine, Rejected},
};

use crate::config::{ConfigPaths, seen::Seen};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SelectedTheme {
    name: String,
    seen: Seen,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfigWatch {
    appearance: WatchedPath,
    keys: Box<WatchedPath>,
    theme: Option<Box<SelectedTheme>>,
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
            keys: Box::new(WatchedPath::starting(
                paths.config.clone(),
                paths.seen.config.as_deref(),
            )),
            theme: paths.theme.clone().map(|name| {
                Box::new(SelectedTheme {
                    name,
                    seen: Seen::starting(paths.seen.theme.as_deref()),
                })
            }),
            themes: paths.themes.clone(),
            theme_list: Seen::Never,
        }
    }

    fn theme_path(&self, name: &str) -> PathBuf {
        self.themes.join(theme_file_name(name))
    }
}

#[derive(Debug)]
pub(crate) enum WatchMessage {
    Poll(ConfigFile),
    PollThemes,
    Observed {
        file: ConfigFile,
        text: Option<String>,
    },
    Unreadable {
        file: ConfigFile,
        detail: String,
    },
    ThemesUnreadable(String),
    Listed(Vec<String>),
    SelectTheme(String),
    Wrote {
        file: ConfigFile,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigWatchError {
    Unselected,
    Selected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigChange {
    Appearance(Option<String>),
    Keymap(Option<String>),
    Theme { name: String, text: Option<String> },
    Themes(Vec<String>),
    Unreadable { file: ConfigFile, detail: String },
    ThemesUnreadable(String),
}

#[derive(Debug, Default)]
pub(crate) enum WatchEffect {
    Read {
        file: ConfigFile,
        path: PathBuf,
    },
    List(PathBuf),
    Changed(ConfigChange),
    #[default]
    Nothing,
}

type Step = Result<(ConfigWatch, WatchEffect), Rejected<ConfigWatch>>;

impl Machine for ConfigWatch {
    type Message = WatchMessage;
    type Error = ConfigWatchError;
    type Effect = WatchEffect;

    fn transition(self, message: WatchMessage) -> Step {
        match message {
            WatchMessage::Poll(ConfigFile::Appearance) => {
                let io = read(ConfigFile::Appearance, &self.appearance.path);
                Ok((self, io))
            }
            WatchMessage::PollThemes => {
                let io = WatchEffect::List(self.themes.clone());
                Ok((self, io))
            }
            WatchMessage::Listed(names) => Ok(self.themes_listed(names)),
            WatchMessage::Poll(ConfigFile::Config) => {
                let io = read(ConfigFile::Config, &self.keys.path);
                Ok((self, io))
            }
            WatchMessage::Poll(ConfigFile::Theme) => self.poll_theme(),
            WatchMessage::Observed {
                file: ConfigFile::Appearance,
                text,
            } => Ok(self.appearance_observed(text)),
            WatchMessage::Observed {
                file: ConfigFile::Config,
                text,
            } => Ok(self.config_observed(text)),
            WatchMessage::Observed {
                file: ConfigFile::Theme,
                text,
            } => self.theme_observed(text),
            WatchMessage::Unreadable { file, detail } => Ok((
                self,
                WatchEffect::Changed(ConfigChange::Unreadable { file, detail }),
            )),
            WatchMessage::ThemesUnreadable(detail) => Ok((
                self,
                WatchEffect::Changed(ConfigChange::ThemesUnreadable(detail)),
            )),
            WatchMessage::SelectTheme(name) => self.select_theme(name),
            WatchMessage::Wrote {
                file: ConfigFile::Appearance,
                text,
            } => Ok(self.appearance_written(&text)),
            WatchMessage::Wrote {
                file: ConfigFile::Config,
                text,
            } => Ok(self.keys_written(&text)),
            WatchMessage::Wrote {
                file: ConfigFile::Theme,
                ..
            } => Ok((self, WatchEffect::Nothing)),
        }
    }
}

impl ConfigWatch {
    fn config_observed(mut self, text: Option<String>) -> (Self, WatchEffect) {
        if !self.keys.seen.changed_by(text.as_deref()) {
            return (self, WatchEffect::Nothing);
        }
        self.keys.seen = Seen::of(text.as_deref());
        (self, WatchEffect::Changed(ConfigChange::Keymap(text)))
    }

    fn keys_written(mut self, text: &str) -> (Self, WatchEffect) {
        self.keys.seen = Seen::of(Some(text));
        (self, WatchEffect::Nothing)
    }

    fn poll_theme(self) -> Step {
        match &self.theme {
            None => Err(Rejected {
                state: self,
                reason: ConfigWatchError::Unselected,
            }),
            Some(theme) => {
                let io = read(ConfigFile::Theme, &self.theme_path(&theme.name));
                Ok((self, io))
            }
        }
    }

    fn appearance_observed(mut self, text: Option<String>) -> (Self, WatchEffect) {
        if !self.appearance.seen.changed_by(text.as_deref()) {
            return (self, WatchEffect::Nothing);
        }
        self.appearance.seen = Seen::of(text.as_deref());
        (self, WatchEffect::Changed(ConfigChange::Appearance(text)))
    }

    fn theme_observed(mut self, text: Option<String>) -> Step {
        let Some(theme) = &mut self.theme else {
            return Err(Rejected {
                state: self,
                reason: ConfigWatchError::Unselected,
            });
        };
        if !theme.seen.changed_by(text.as_deref()) {
            return Ok((self, WatchEffect::Nothing));
        }
        theme.seen = Seen::of(text.as_deref());
        let name = theme.name.clone();
        Ok((
            self,
            WatchEffect::Changed(ConfigChange::Theme { name, text }),
        ))
    }

    fn select_theme(mut self, name: String) -> Step {
        if self.theme.as_ref().is_some_and(|theme| theme.name == name) {
            return Err(Rejected {
                state: self,
                reason: ConfigWatchError::Selected,
            });
        }
        let io = read(ConfigFile::Theme, &self.theme_path(&name));
        self.theme = Some(Box::new(SelectedTheme {
            name,
            seen: Seen::Never,
        }));
        Ok((self, io))
    }

    fn themes_listed(mut self, names: Vec<String>) -> (Self, WatchEffect) {
        let listing = names.join("\n");
        if !self.theme_list.changed_by(Some(&listing)) {
            return (self, WatchEffect::Nothing);
        }
        self.theme_list = Seen::of(Some(&listing));
        (self, WatchEffect::Changed(ConfigChange::Themes(names)))
    }

    fn appearance_written(mut self, text: &str) -> (Self, WatchEffect) {
        self.appearance.seen = Seen::of(Some(text));
        (self, WatchEffect::Nothing)
    }
}

fn read(file: ConfigFile, path: &Path) -> WatchEffect {
    WatchEffect::Read {
        file,
        path: path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use kernel::{domain::ConfigFile, update::Machine};
    use rstest::rstest;

    use crate::config::{
        ConfigPaths,
        SeenTexts,
        watch::{
            ConfigChange,
            ConfigWatch,
            ConfigWatchError,
            WatchEffect,
            WatchMessage,
        },
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
    #[case(ConfigFile::Appearance, "a = 1\n")]
    #[case(ConfigFile::Theme, "b = 2\n")]
    #[case(ConfigFile::Config, "c = 3\n")]
    fn a_seen_start_text_is_not_reported_on_the_first_poll(
        #[case] file: ConfigFile,
        #[case] text: &str,
    ) {
        let watch = seen_at_start("a = 1\n", "b = 2\n", "c = 3\n");
        let (_, io) = step(watch, observed(file, Some(text)));
        assert!(is_nothing(&io));
    }

    #[rstest]
    #[case(ConfigFile::Appearance, "a = 1 \n")]
    #[case(ConfigFile::Theme, "b = 3\n")]
    #[case(ConfigFile::Config, "c = 3")]
    fn a_changed_file_after_start_is_reported(
        #[case] file: ConfigFile,
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

    fn observed(file: ConfigFile, text: Option<&str>) -> WatchMessage {
        WatchMessage::Observed {
            file,
            text: text.map(str::to_string),
        }
    }

    type Pair = (ConfigWatch, WatchEffect);

    fn step(state: ConfigWatch, message: WatchMessage) -> Pair {
        match state.transition(message) {
            Ok(pair) => pair,
            Err(rejected) => (rejected.state, WatchEffect::Nothing),
        }
    }

    fn reads(io: &WatchEffect, file: ConfigFile, path: &str) -> bool {
        match io {
            WatchEffect::Read {
                file: read_file,
                path: read_path,
            } => *read_file == file && read_path == Path::new(path),
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
    #[case(ConfigFile::Appearance, UI_PATH)]
    #[case(ConfigFile::Config, KEYS_PATH)]
    #[case(ConfigFile::Theme, "/config/themes/noir.toml")]
    fn poll_asks_for_the_file_it_watches(#[case] file: ConfigFile, #[case] path: &str) {
        let (_, io) = step(selected("noir"), WatchMessage::Poll(file));
        assert!(reads(&io, file, path));
    }

    #[rstest]
    #[case(WatchMessage::Poll(ConfigFile::Theme))]
    #[case(observed(ConfigFile::Theme, Some("name = \"noir\"")))]
    fn the_theme_target_refuses_while_unselected(#[case] message: WatchMessage) {
        let refused = unselected().transition(message).err();
        let (state, reason) = refused
            .map(|rejected| (rejected.state, rejected.reason))
            .unzip();
        assert_eq!(state, Some(unselected()));
        assert_eq!(reason, Some(ConfigWatchError::Unselected));
    }

    #[rstest]
    #[case(ConfigFile::Appearance, Some("[window]\nkey_hints = false\n"))]
    #[case(ConfigFile::Appearance, None)]
    #[case(ConfigFile::Config, Some("[keymap]\nnext = \"x\"\n"))]
    #[case(ConfigFile::Config, None)]
    #[case(ConfigFile::Theme, Some("name = \"noir\"\n"))]
    #[case(ConfigFile::Theme, None)]
    fn the_first_observation_is_reported(
        #[case] file: ConfigFile,
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
                assert_eq!(name, "noir");
                content
            }
            ConfigChange::Themes(_)
            | ConfigChange::Unreadable { .. }
            | ConfigChange::ThemesUnreadable(_) => None,
        };
        assert_eq!(reported.as_deref(), text);
    }

    #[rstest]
    #[case(ConfigFile::Appearance, Some("[window]\nkey_hints = false\n"))]
    #[case(ConfigFile::Appearance, None)]
    #[case(ConfigFile::Config, Some("[keymap]\nnext = \"x\"\n"))]
    #[case(ConfigFile::Config, None)]
    #[case(ConfigFile::Theme, Some("name = \"noir\"\n"))]
    #[case(ConfigFile::Theme, None)]
    fn an_unchanged_observation_reports_nothing(
        #[case] file: ConfigFile,
        #[case] text: Option<&str>,
    ) {
        let (settled, _) = step(selected("noir"), observed(file, text));
        let (_, io) = step(settled, observed(file, text));
        assert!(is_nothing(&io));
    }

    #[rstest]
    #[case(
        ConfigFile::Appearance,
        "[window]\nkey_hints = false\n",
        "[window]\nkey_hints = true\n"
    )]
    #[case(
        ConfigFile::Config,
        "[keymap]\nnext = \"x\"\n",
        "[keymap]\nnext = \"y\"\n"
    )]
    fn a_changed_observation_is_reported_again(
        #[case] file: ConfigFile,
        #[case] first: &str,
        #[case] second: &str,
    ) {
        let (settled, _) = step(selected("noir"), observed(file, Some(first)));
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
    #[case(ConfigFile::Appearance)]
    #[case(ConfigFile::Config)]
    fn the_drivers_own_write_is_not_reported_back(#[case] file: ConfigFile) {
        let written = "written\n";
        let wrote = WatchMessage::Wrote {
            file,
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
        assert!(reads(&io, ConfigFile::Theme, "/config/themes/wafer.toml"));
        assert_eq!(state, selected("wafer"));
    }

    #[test]
    fn a_reselected_theme_with_no_file_still_reports() {
        let (state, _) = step(
            selected("noir"),
            WatchMessage::SelectTheme("wafer".to_string()),
        );
        let (_, io) = step(state, observed(ConfigFile::Theme, None));
        assert_eq!(
            sent(io),
            Some(ConfigChange::Theme {
                name: "wafer".to_string(),
                text: None,
            })
        );
    }

    #[test]
    fn selecting_the_current_theme_is_refused() {
        let refused = selected("noir")
            .transition(WatchMessage::SelectTheme("noir".to_string()))
            .err();
        let (state, reason) = refused
            .map(|rejected| (rejected.state, rejected.reason))
            .unzip();
        assert_eq!(state, Some(selected("noir")));
        assert_eq!(reason, Some(ConfigWatchError::Selected));
    }

    #[test]
    fn selecting_a_theme_while_unselected_arms_the_target() {
        let (state, io) =
            step(unselected(), WatchMessage::SelectTheme("noir".to_string()));
        assert!(reads(&io, ConfigFile::Theme, "/config/themes/noir.toml"));
        assert_eq!(state, selected("noir"));
    }

    #[test]
    fn a_keys_file_that_cannot_be_read_is_reported_as_no_text() {
        let (settled, _) = step(
            selected("noir"),
            observed(ConfigFile::Config, Some("[keymap]\nnext = \"x\"\n")),
        );
        let (_, io) = step(settled, observed(ConfigFile::Config, None));
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
    #[case("permission denied")]
    fn an_unreadable_file_is_reported_without_being_marked_seen(#[case] error: &str) {
        let (state, io) = step(
            selected("noir"),
            WatchMessage::Unreadable {
                file: ConfigFile::Theme,
                detail: error.to_string(),
            },
        );
        assert_eq!(
            sent(io),
            Some(ConfigChange::Unreadable {
                file: ConfigFile::Theme,
                detail: error.to_string(),
            })
        );
        assert_eq!(state, selected("noir"));
    }
}
