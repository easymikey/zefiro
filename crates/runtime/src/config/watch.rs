use std::path::{Path, PathBuf};

use config::theme_file_name;
use kernel::{
    domain::ConfigFile,
    update::{Machine, Rejected},
};

use crate::config::{ConfigPaths, seen::Seen};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchedFile {
    Appearance,
    Keys,
    Theme,
}

pub(crate) fn config_file(file: WatchedFile) -> ConfigFile {
    match file {
        WatchedFile::Appearance => ConfigFile::Appearance,
        WatchedFile::Keys => ConfigFile::Keymap,
        WatchedFile::Theme => ConfigFile::Theme,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WatchedPath {
    path: PathBuf,
    seen: Seen,
}

impl WatchedPath {
    fn fresh(path: PathBuf) -> Self {
        Self {
            path,
            seen: Seen::Fresh,
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
    keys: Option<Box<WatchedPath>>,
    theme: Option<Box<SelectedTheme>>,
    themes: PathBuf,
    theme_list: Seen,
}

impl ConfigWatch {
    #[must_use]
    pub(crate) fn new(paths: &ConfigPaths) -> Self {
        Self {
            appearance: WatchedPath::fresh(paths.appearance.clone()),
            keys: paths
                .config
                .clone()
                .map(|path| Box::new(WatchedPath::fresh(path))),
            theme: paths.theme.clone().map(|name| {
                Box::new(SelectedTheme {
                    name,
                    seen: Seen::Fresh,
                })
            }),
            themes: paths.themes.clone(),
            theme_list: Seen::Fresh,
        }
    }

    fn theme_path(&self, name: &str) -> PathBuf {
        self.themes.join(theme_file_name(name))
    }
}

#[derive(Debug)]
pub(crate) enum ConfigWatchMessage {
    Poll(WatchedFile),
    PollThemes,
    Observed {
        file: WatchedFile,
        text: Option<String>,
    },
    Unreadable {
        file: ConfigFile,
        detail: String,
    },
    Listed(Vec<String>),
    SelectTheme(String),
    WroteAppearance(String),
    WroteKeys(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigWatchRejection {
    Unselected,
    Selected,
    Unwatched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigChange {
    Appearance(Option<String>),
    Keymap(Option<String>),
    Theme { name: String, text: Option<String> },
    Themes(Vec<String>),
    Unreadable { file: ConfigFile, detail: String },
}

#[derive(Debug, Default)]
pub(crate) enum ConfigIo {
    Read {
        file: WatchedFile,
        path: PathBuf,
    },
    List(PathBuf),
    Send(ConfigChange),
    #[default]
    Nothing,
}

type Step = Result<(ConfigWatch, ConfigIo), Rejected<ConfigWatch>>;

impl Machine for ConfigWatch {
    type Message = ConfigWatchMessage;
    type Rejection = ConfigWatchRejection;
    type Effect = ConfigIo;

    fn transition(self, message: ConfigWatchMessage) -> Step {
        match message {
            ConfigWatchMessage::Poll(WatchedFile::Appearance) => {
                let io = read(WatchedFile::Appearance, &self.appearance.path);
                Ok((self, io))
            }
            ConfigWatchMessage::PollThemes => {
                let io = ConfigIo::List(self.themes.clone());
                Ok((self, io))
            }
            ConfigWatchMessage::Listed(names) => Ok(self.themes_listed(names)),
            ConfigWatchMessage::Poll(WatchedFile::Keys) => self.poll_keys(),
            ConfigWatchMessage::Poll(WatchedFile::Theme) => self.poll_theme(),
            ConfigWatchMessage::Observed {
                file: WatchedFile::Appearance,
                text,
            } => Ok(self.appearance_observed(text)),
            ConfigWatchMessage::Observed {
                file: WatchedFile::Keys,
                text,
            } => self.keys_observed(text),
            ConfigWatchMessage::Observed {
                file: WatchedFile::Theme,
                text,
            } => self.theme_observed(text),
            ConfigWatchMessage::Unreadable { file, detail } => Ok((
                self,
                ConfigIo::Send(ConfigChange::Unreadable { file, detail }),
            )),
            ConfigWatchMessage::SelectTheme(name) => self.select_theme(name),
            ConfigWatchMessage::WroteAppearance(text) => {
                Ok(self.appearance_written(&text))
            }
            ConfigWatchMessage::WroteKeys(text) => Ok(self.keys_written(&text)),
        }
    }
}

impl ConfigWatch {
    fn poll_keys(self) -> Step {
        match &self.keys {
            None => Err(Rejected {
                state: self,
                reason: ConfigWatchRejection::Unwatched,
            }),
            Some(keys) => {
                let io = read(WatchedFile::Keys, &keys.path);
                Ok((self, io))
            }
        }
    }

    fn keys_observed(mut self, text: Option<String>) -> Step {
        let Some(keys) = &mut self.keys else {
            return Err(Rejected {
                state: self,
                reason: ConfigWatchRejection::Unwatched,
            });
        };
        if !keys.seen.changed_by(text.as_deref()) {
            return Ok((self, ConfigIo::Nothing));
        }
        keys.seen = Seen::of(text.as_deref());
        Ok((self, ConfigIo::Send(ConfigChange::Keymap(text))))
    }

    fn keys_written(mut self, text: &str) -> (Self, ConfigIo) {
        if let Some(keys) = &mut self.keys {
            keys.seen = Seen::of(Some(text));
        }
        (self, ConfigIo::Nothing)
    }

    fn poll_theme(self) -> Step {
        match &self.theme {
            None => Err(Rejected {
                state: self,
                reason: ConfigWatchRejection::Unselected,
            }),
            Some(theme) => {
                let io = read(WatchedFile::Theme, &self.theme_path(&theme.name));
                Ok((self, io))
            }
        }
    }

    fn appearance_observed(mut self, text: Option<String>) -> (Self, ConfigIo) {
        if !self.appearance.seen.changed_by(text.as_deref()) {
            return (self, ConfigIo::Nothing);
        }
        self.appearance.seen = Seen::of(text.as_deref());
        (self, ConfigIo::Send(ConfigChange::Appearance(text)))
    }

    fn theme_observed(mut self, text: Option<String>) -> Step {
        let Some(theme) = &mut self.theme else {
            return Err(Rejected {
                state: self,
                reason: ConfigWatchRejection::Unselected,
            });
        };
        if !theme.seen.changed_by(text.as_deref()) {
            return Ok((self, ConfigIo::Nothing));
        }
        theme.seen = Seen::of(text.as_deref());
        let name = theme.name.clone();
        Ok((self, ConfigIo::Send(ConfigChange::Theme { name, text })))
    }

    fn select_theme(mut self, name: String) -> Step {
        if self.theme.as_ref().is_some_and(|theme| theme.name == name) {
            return Err(Rejected {
                state: self,
                reason: ConfigWatchRejection::Selected,
            });
        }
        let io = read(WatchedFile::Theme, &self.theme_path(&name));
        self.theme = Some(Box::new(SelectedTheme {
            name,
            seen: Seen::Fresh,
        }));
        Ok((self, io))
    }

    fn themes_listed(mut self, names: Vec<String>) -> (Self, ConfigIo) {
        let listing = names.join("\n");
        if !self.theme_list.changed_by(Some(&listing)) {
            return (self, ConfigIo::Nothing);
        }
        self.theme_list = Seen::of(Some(&listing));
        (self, ConfigIo::Send(ConfigChange::Themes(names)))
    }

    fn appearance_written(mut self, text: &str) -> (Self, ConfigIo) {
        self.appearance.seen = Seen::of(Some(text));
        (self, ConfigIo::Nothing)
    }
}

fn read(file: WatchedFile, path: &Path) -> ConfigIo {
    ConfigIo::Read {
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
        watch::{
            ConfigChange,
            ConfigIo,
            ConfigWatch,
            ConfigWatchMessage,
            ConfigWatchRejection,
            WatchedFile,
        },
    };

    const UI_PATH: &str = "/config/sifr-ui.toml";
    const THEMES_DIR: &str = "/config/themes";
    const KEYS_PATH: &str = "/config/config.toml";

    fn paths(keys: Option<&str>, theme: Option<&str>) -> ConfigPaths {
        ConfigPaths {
            config: keys.map(PathBuf::from),
            appearance: PathBuf::from(UI_PATH),
            themes: PathBuf::from(THEMES_DIR),
            theme: theme.map(str::to_string),
        }
    }

    fn unselected() -> ConfigWatch {
        ConfigWatch::new(&paths(Some(KEYS_PATH), None))
    }

    fn selected(name: &str) -> ConfigWatch {
        ConfigWatch::new(&paths(Some(KEYS_PATH), Some(name)))
    }

    fn unwatched_keys() -> ConfigWatch {
        ConfigWatch::new(&paths(None, Some("noir")))
    }

    fn observed(file: WatchedFile, text: Option<&str>) -> ConfigWatchMessage {
        ConfigWatchMessage::Observed {
            file,
            text: text.map(str::to_string),
        }
    }

    type Pair = (ConfigWatch, ConfigIo);

    fn step(state: ConfigWatch, message: ConfigWatchMessage) -> Pair {
        match state.transition(message) {
            Ok(pair) => pair,
            Err(rejected) => (rejected.state, ConfigIo::Nothing),
        }
    }

    fn reads(io: &ConfigIo, file: WatchedFile, path: &str) -> bool {
        match io {
            ConfigIo::Read {
                file: read_file,
                path: read_path,
            } => *read_file == file && read_path == Path::new(path),
            ConfigIo::List(_) | ConfigIo::Send(_) | ConfigIo::Nothing => false,
        }
    }

    fn sent(io: ConfigIo) -> Option<ConfigChange> {
        match io {
            ConfigIo::Send(change) => Some(change),
            ConfigIo::Read { .. } | ConfigIo::List(_) | ConfigIo::Nothing => None,
        }
    }

    fn is_nothing(io: &ConfigIo) -> bool {
        matches!(io, ConfigIo::Nothing)
    }

    #[rstest]
    #[case(WatchedFile::Appearance, UI_PATH)]
    #[case(WatchedFile::Keys, KEYS_PATH)]
    #[case(WatchedFile::Theme, "/config/themes/noir.toml")]
    fn poll_asks_for_the_file_it_watches(
        #[case] file: WatchedFile,
        #[case] path: &str,
    ) {
        let (_, io) = step(selected("noir"), ConfigWatchMessage::Poll(file));
        assert!(reads(&io, file, path));
    }

    #[rstest]
    #[case(ConfigWatchMessage::Poll(WatchedFile::Theme))]
    #[case(observed(WatchedFile::Theme, Some("name = \"noir\"")))]
    fn the_theme_target_refuses_while_unselected(#[case] message: ConfigWatchMessage) {
        let refused = unselected().transition(message).err();
        let (state, reason) = refused
            .map(|rejected| (rejected.state, rejected.reason))
            .unzip();
        assert_eq!(state, Some(unselected()));
        assert_eq!(reason, Some(ConfigWatchRejection::Unselected));
    }

    #[rstest]
    #[case(WatchedFile::Appearance, Some("[window]\nkey_hints = false\n"))]
    #[case(WatchedFile::Appearance, None)]
    #[case(WatchedFile::Keys, Some("[keymap]\nnext = \"x\"\n"))]
    #[case(WatchedFile::Keys, None)]
    #[case(WatchedFile::Theme, Some("name = \"noir\"\n"))]
    #[case(WatchedFile::Theme, None)]
    fn the_first_observation_is_reported(
        #[case] file: WatchedFile,
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
            ConfigChange::Themes(_) | ConfigChange::Unreadable { .. } => None,
        };
        assert_eq!(reported.as_deref(), text);
    }

    #[rstest]
    #[case(WatchedFile::Appearance, Some("[window]\nkey_hints = false\n"))]
    #[case(WatchedFile::Appearance, None)]
    #[case(WatchedFile::Keys, Some("[keymap]\nnext = \"x\"\n"))]
    #[case(WatchedFile::Keys, None)]
    #[case(WatchedFile::Theme, Some("name = \"noir\"\n"))]
    #[case(WatchedFile::Theme, None)]
    fn an_unchanged_observation_reports_nothing(
        #[case] file: WatchedFile,
        #[case] text: Option<&str>,
    ) {
        let (settled, _) = step(selected("noir"), observed(file, text));
        let (_, io) = step(settled, observed(file, text));
        assert!(is_nothing(&io));
    }

    #[rstest]
    #[case(
        WatchedFile::Appearance,
        "[window]\nkey_hints = false\n",
        "[window]\nkey_hints = true\n"
    )]
    #[case(
        WatchedFile::Keys,
        "[keymap]\nnext = \"x\"\n",
        "[keymap]\nnext = \"y\"\n"
    )]
    fn a_changed_observation_is_reported_again(
        #[case] file: WatchedFile,
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
            | ConfigChange::Unreadable { .. } => None,
        };
        assert_eq!(content.as_deref(), Some(second));
    }

    fn wrote_appearance() -> fn(String) -> ConfigWatchMessage {
        ConfigWatchMessage::WroteAppearance
    }

    fn wrote_keys() -> fn(String) -> ConfigWatchMessage {
        ConfigWatchMessage::WroteKeys
    }

    #[rstest]
    #[case(WatchedFile::Appearance, wrote_appearance())]
    #[case(WatchedFile::Keys, wrote_keys())]
    fn the_drivers_own_write_is_not_reported_back(
        #[case] file: WatchedFile,
        #[case] wrote: fn(String) -> ConfigWatchMessage,
    ) {
        let written = "written\n";
        let (after_write, io) = step(selected("noir"), wrote(written.to_string()));
        assert!(is_nothing(&io));
        let (_, io_after_observe) = step(after_write, observed(file, Some(written)));
        assert!(is_nothing(&io_after_observe));
    }

    #[test]
    fn selecting_another_theme_repoints_and_reads_at_once() {
        let (state, io) = step(
            selected("noir"),
            ConfigWatchMessage::SelectTheme("wafer".to_string()),
        );
        assert!(reads(&io, WatchedFile::Theme, "/config/themes/wafer.toml"));
        assert_eq!(state, selected("wafer"));
    }

    #[test]
    fn a_reselected_theme_with_no_file_still_reports() {
        let (state, _) = step(
            selected("noir"),
            ConfigWatchMessage::SelectTheme("wafer".to_string()),
        );
        let (_, io) = step(state, observed(WatchedFile::Theme, None));
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
            .transition(ConfigWatchMessage::SelectTheme("noir".to_string()))
            .err();
        let (state, reason) = refused
            .map(|rejected| (rejected.state, rejected.reason))
            .unzip();
        assert_eq!(state, Some(selected("noir")));
        assert_eq!(reason, Some(ConfigWatchRejection::Selected));
    }

    #[test]
    fn selecting_a_theme_while_unselected_arms_the_target() {
        let (state, io) = step(
            unselected(),
            ConfigWatchMessage::SelectTheme("noir".to_string()),
        );
        assert!(reads(&io, WatchedFile::Theme, "/config/themes/noir.toml"));
        assert_eq!(state, selected("noir"));
    }

    #[test]
    fn a_keys_file_that_cannot_be_read_is_reported_as_no_text() {
        let (settled, _) = step(
            selected("noir"),
            observed(WatchedFile::Keys, Some("[keymap]\nnext = \"x\"\n")),
        );
        let (_, io) = step(settled, observed(WatchedFile::Keys, None));
        assert_eq!(sent(io), Some(ConfigChange::Keymap(None)));
    }

    fn listed(names: &[&str]) -> ConfigWatchMessage {
        ConfigWatchMessage::Listed(
            names.iter().map(|name| (*name).to_string()).collect(),
        )
    }

    fn themes(io: ConfigIo) -> Option<Vec<String>> {
        match sent(io) {
            Some(ConfigChange::Themes(names)) => Some(names),
            Some(
                ConfigChange::Appearance(_)
                | ConfigChange::Keymap(_)
                | ConfigChange::Theme { .. }
                | ConfigChange::Unreadable { .. },
            )
            | None => None,
        }
    }

    #[test]
    fn polling_asks_for_the_themes_directory() {
        let (_, io) = step(selected("noir"), ConfigWatchMessage::PollThemes);
        assert!(matches!(io, ConfigIo::List(dir) if dir == Path::new(THEMES_DIR)));
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
    #[case(ConfigWatchMessage::Poll(WatchedFile::Keys))]
    #[case(observed(WatchedFile::Keys, Some("[keymap]\nnext = \"x\"\n")))]
    #[case(ConfigWatchMessage::WroteKeys("[keymap]\nnext = \"x\"\n".to_string()))]
    fn a_config_file_that_does_not_exist_is_never_watched(
        #[case] message: ConfigWatchMessage,
    ) {
        let (state, io) = step(unwatched_keys(), message);
        assert!(is_nothing(&io));
        assert_eq!(state, unwatched_keys());
    }

    #[rstest]
    #[case("permission denied")]
    fn an_unreadable_file_is_reported_without_being_marked_seen(#[case] error: &str) {
        let (state, io) = step(
            selected("noir"),
            ConfigWatchMessage::Unreadable {
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
