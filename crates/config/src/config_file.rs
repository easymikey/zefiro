use std::{path::PathBuf, time::Duration};

use kernel::domain::{
    config::ConfigName,
    crossfade::Crossfade,
    device::{DeviceName, OutputDevice},
    keymap::KeymapOverrides,
    percent::Percent,
    settings::{AudioSettings, ReplayGain},
    sleep_presets::SleepPresets,
    theme::ThemeChoice,
    transport::Transport,
};
use serde::{Deserialize, Deserializer};

use crate::{
    appearance::{Flag, flag},
    error::{CrossfadeError, Error, parse_toml},
    keymap::TomlKeymap,
};

pub const CONFIG_FILE_NAME: &str = "config.toml";

fn parse_crossfade(raw: &str) -> Result<Crossfade, CrossfadeError> {
    let trimmed = raw.trim();
    let duration = if let Some(milliseconds) = trimmed.strip_suffix("ms") {
        milliseconds
            .trim()
            .parse::<u64>()
            .map(Duration::from_millis)
            .map_err(CrossfadeError::Number)?
    } else {
        let Some(seconds) = trimmed.strip_suffix('s') else {
            return Err(CrossfadeError::MissingSuffix);
        };
        seconds
            .trim()
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(CrossfadeError::Number)?
    };
    Ok(Crossfade::try_from(duration)?)
}

fn crossfade<'de, D>(deserializer: D) -> Result<Crossfade, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    parse_crossfade(&raw).map_err(serde::de::Error::custom)
}

fn theme<'de, D>(deserializer: D) -> Result<ThemeChoice, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(serde::de::Error::custom)
}

fn volume<'de, D>(deserializer: D) -> Result<Percent, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = u8::deserialize(deserializer)?;
    Percent::new(raw).ok_or_else(|| {
        serde::de::Error::custom(format!(
            "volume {raw} is out of range (must be 0..=100)"
        ))
    })
}

fn sleep_presets<'de, D>(deserializer: D) -> Result<SleepPresets, D::Error>
where
    D: Deserializer<'de>,
{
    let minutes = Vec::<u64>::deserialize(deserializer)?;
    SleepPresets::from_minutes(&minutes).map_err(serde::de::Error::custom)
}

impl Flag for ReplayGain {
    const ON: Self = Self::On;
    const OFF: Self = Self::Off;
}

fn device<'de, D>(deserializer: D) -> Result<OutputDevice, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(DeviceName::new)
        .transpose()
        .map_err(serde::de::Error::custom)
        .map(|name| name.map_or(OutputDevice::SystemDefault, OutputDevice::Named))
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, deny_unknown_fields, expecting = "an [audio] table")]
pub struct TomlAudio {
    #[serde(deserialize_with = "crossfade")]
    pub(crate) crossfade: Crossfade,
    #[serde(deserialize_with = "flag")]
    #[serde(rename = "replaygain")]
    pub(crate) replay_gain: ReplayGain,
    #[serde(deserialize_with = "device")]
    pub(crate) device: OutputDevice,
    #[serde(deserialize_with = "sleep_presets")]
    pub(crate) sleep_presets: SleepPresets,
}

impl From<TomlAudio> for AudioSettings {
    fn from(config: TomlAudio) -> Self {
        Self {
            crossfade: config.crossfade,
            replay_gain: config.replay_gain,
            device: config.device,
            sleep_presets: config.sleep_presets,
        }
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TomlConfig {
    pub music_dir: Option<PathBuf>,
    #[serde(deserialize_with = "theme")]
    pub theme: ThemeChoice,
    #[serde(deserialize_with = "volume")]
    pub volume: Percent,
    pub audio: TomlAudio,
    pub(crate) keymap: TomlKeymap,
}

impl Default for TomlConfig {
    fn default() -> Self {
        Self {
            music_dir: None,
            theme: ThemeChoice::default(),
            volume: Transport::default().volume,
            audio: TomlAudio::default(),
            keymap: TomlKeymap::default(),
        }
    }
}

pub fn parse_config(text: &str) -> Result<TomlConfig, Error> {
    parse_toml(text, ConfigName::Config)
}

#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSettings {
    pub keymap: KeymapOverrides,
    pub(crate) music_dir: Option<PathBuf>,
}

pub fn parse_config_reload(text: &str) -> Result<ConfigSettings, Error> {
    parse_config(text).map(|config| ConfigSettings {
        keymap: config
            .keymap
            .0
            .into_iter()
            .map(|(action, binding)| (action, binding.0))
            .collect(),
        music_dir: config.music_dir,
    })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use kernel::domain::{
        bounded::Bounded,
        crossfade::Crossfade,
        keymap::{Action, KeyOverride, KeymapOverrides},
        theme::{ThemeChoice, ThemeName},
    };
    use rstest::rstest;

    use crate::{
        config_file::{ConfigSettings, parse_config, parse_config_reload},
        error::Error,
    };

    #[rstest]
    #[case::an_empty_file("defaults", "")]
    #[case::sleep_presets("sleep_presets", "[audio]\nsleep_presets = [10, 20]\n")]
    #[case::a_partial_audio_table("partial_audio", "[audio]\ncrossfade = \"3s\"\n")]
    #[case::a_named_device("device", "[audio]\ndevice = \"Speakers\"\n")]
    #[case::key_overrides("keymap", "[keymap]\nplay_pause = \"space\"\nquit = \"q\"\n")]
    #[case::a_key_in_a_context(
        "keymap_in_a_context",
        "[keymap]\nnext = { chord = \"ctrl+n\", context = \"search\" }\n"
    )]
    #[case::a_malformed_chord("malformed_chord", "[keymap]\nnext = \"not-a-key\"\n")]
    fn config_parse_reads_every_table(#[case] name: &str, #[case] text: &str) {
        let config = parse_config(text).unwrap();
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_debug_snapshot!(config);
        });
    }

    #[test]
    fn invalid_toml_becomes_a_typed_parse_error() {
        let parsed = parse_config("volume = \"not-a-number\"");
        assert!(matches!(parsed, Err(Error::Parse { .. })));
    }

    #[rstest]
    #[case::an_unknown_top_level_table("unknown_table", "[nope]\nkey = 1\n")]
    #[case::an_unknown_key_in_a_known_table("unknown_key", "[audio]\nbogus = 1\n")]
    fn unknown_toml_names_the_key(#[case] name: &str, #[case] text: &str) {
        let error = parse_config(text).expect_err("unknown TOML must not parse");
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(error.to_string());
        });
    }

    #[test]
    fn a_configured_music_dir_is_kept_as_written() {
        let config = parse_config("music_dir = \"~/Music\"\n").unwrap();
        assert_eq!(config.music_dir, Some("~/Music".into()));
    }

    #[rstest]
    #[case::gapless("0s", Crossfade::default())]
    #[case::seconds("3s", Crossfade::clamped(Duration::from_secs(3)))]
    #[case::milliseconds("250ms", Crossfade::clamped(Duration::from_millis(250)))]
    fn crossfade_reads_an_integer_with_a_time_suffix(
        #[case] spelling: &str,
        #[case] expected: Crossfade,
    ) {
        let text = format!("[audio]\ncrossfade = \"{spelling}\"\n");
        let config = parse_config(&text).unwrap();
        assert_eq!(config.audio.crossfade, expected);
    }

    #[rstest]
    #[case::out_of_range("11s")]
    #[case::missing_suffix("3")]
    #[case::unknown_suffix("3x")]
    #[case::not_a_number("abcs")]
    fn crossfade_rejects_what_it_cannot_place(#[case] spelling: &str) {
        let text = format!("[audio]\ncrossfade = \"{spelling}\"\n");
        assert!(
            matches!(parse_config(&text), Err(Error::Parse { .. })),
            "{spelling:?} must not parse"
        );
    }

    #[test]
    fn sleep_presets_are_read_as_whole_minutes() {
        let config = parse_config("[audio]\nsleep_presets = [10, 20]\n").unwrap();
        assert_eq!(
            config.audio.sleep_presets.as_slice(),
            [Duration::from_secs(600), Duration::from_secs(1200)]
        );
    }

    #[rstest]
    #[case::volume_101("volume = 101")]
    #[case::volume_negative("volume = -1")]
    #[case::theme_empty("theme = \"\"")]
    #[case::sleep_zero_minutes("[audio]\nsleep_presets = [0]\n")]
    #[case::sleep_not_ascending("[audio]\nsleep_presets = [30, 20]\n")]
    #[case::sleep_too_many("[audio]\nsleep_presets = [1, 2, 3, 4, 5, 6]\n")]
    #[case::device_empty("[audio]\ndevice = \"\"\n")]
    fn config_values_out_of_range_are_rejected(#[case] text: &str) {
        assert!(
            matches!(parse_config(text), Err(Error::Parse { .. })),
            "{text:?} must not parse"
        );
    }

    #[rstest]
    #[case::volume_0("volume = 0")]
    #[case::volume_100("volume = 100")]
    #[case::theme_auto("theme = \"auto\"")]
    #[case::theme_named("theme = \"AUTO\"")]
    #[case::sleep_empty_means_off("[audio]\nsleep_presets = []\n")]
    #[case::sleep_three_presets("[audio]\nsleep_presets = [1, 360, 720]\n")]
    #[case::device_named("[audio]\ndevice = \"Speakers\"\n")]
    #[case::device_absent("")]
    fn config_values_in_range_are_kept(#[case] text: &str) {
        assert!(parse_config(text).is_ok(), "{text:?} must parse");
    }

    #[test]
    fn a_named_theme_is_parsed_case_sensitively() {
        let config = parse_config("theme = \"AUTO\"\n").unwrap();
        assert_eq!(
            config.theme,
            ThemeChoice::Named(ThemeName::from_static("AUTO"))
        );
    }

    #[test]
    fn keymap_error_names_the_file_and_the_line() {
        let Err(error) = parse_config_reload("[keymap]\nnext = \"x\"\n[keymap]\n")
        else {
            panic!("a broken config file must not parse");
        };
        let text = error.to_string();
        assert_eq!(text.lines().nth(1), Some("config.toml:3"), "was {text:?}");
    }

    #[test]
    fn a_config_file_yields_its_keymap_and_its_music_dir_and_ignores_other_tables() {
        let parsed = parse_config_reload(
            "music_dir = \"/tmp\"\ntheme = \"dark\"\n\n[audio]\ncrossfade = \"3s\"\n\n[keymap]\nnext = \"x\"\n",
        );
        assert_eq!(
            parsed.ok(),
            Some(ConfigSettings {
                keymap: KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]),
                music_dir: Some(PathBuf::from("/tmp")),
            })
        );
    }

    #[test]
    fn a_broken_config_file_reports_a_parse_error() {
        assert!(matches!(
            parse_config_reload("[keymap\nnot toml"),
            Err(Error::Parse { .. })
        ));
    }
}
