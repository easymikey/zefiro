use std::{path::PathBuf, time::Duration};

use kernel::domain::{Crossfade, Replaygain, Settings, Transport};
use serde::{Deserialize, Deserializer};

use crate::{
    appearance::two_state,
    error::{ConfigError, CrossfadeRejection, named_toml},
    keymap::KeymapFile,
};

pub const CONFIG_FILE_NAME: &str = "config.toml";

fn default_theme() -> String {
    "auto".to_string()
}

fn default_volume() -> u8 {
    Transport::default().volume.value()
}

fn default_sleep_presets() -> Vec<Duration> {
    Settings::default().sleep_presets.into_vec()
}

fn parse_crossfade(raw: &str) -> Result<Crossfade, CrossfadeRejection> {
    let trimmed = raw.trim();
    let number = |source| CrossfadeRejection::Number {
        value: raw.to_string(),
        source,
    };
    let duration = if let Some(milliseconds) = trimmed.strip_suffix("ms") {
        milliseconds
            .trim()
            .parse::<u64>()
            .map(Duration::from_millis)
            .map_err(number)?
    } else {
        let Some(seconds) = trimmed.strip_suffix('s') else {
            return Err(CrossfadeRejection::MissingSuffix {
                value: raw.to_string(),
            });
        };
        seconds
            .trim()
            .parse::<u64>()
            .map(Duration::from_secs)
            .map_err(number)?
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

fn sleep_presets<'de, D>(deserializer: D) -> Result<Vec<Duration>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<u64>::deserialize(deserializer)
        .map(|minutes| minutes.into_iter().map(Duration::from_mins).collect())
}

fn replaygain<'de, D>(deserializer: D) -> Result<Replaygain, D::Error>
where
    D: Deserializer<'de>,
{
    two_state(deserializer, Replaygain::On, Replaygain::Off)
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    #[serde(deserialize_with = "crossfade")]
    pub crossfade: Crossfade,
    #[serde(deserialize_with = "replaygain")]
    pub replaygain: Replaygain,
    pub device: Option<String>,
    #[serde(deserialize_with = "sleep_presets")]
    pub sleep_presets: Vec<Duration>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            crossfade: Crossfade::default(),
            replaygain: Replaygain::Off,
            device: None,
            sleep_presets: default_sleep_presets(),
        }
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfigFile {
    pub music_dir: Option<PathBuf>,
    pub theme: String,
    pub volume: u8,
    pub audio: AudioConfig,
    pub keymap: KeymapFile,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            music_dir: None,
            theme: default_theme(),
            volume: default_volume(),
            audio: AudioConfig::default(),
            keymap: KeymapFile::default(),
        }
    }
}

pub fn parse(text: &str) -> Result<ConfigFile, ConfigError> {
    named_toml(text, CONFIG_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{Bounded, domain::Crossfade};
    use rstest::rstest;

    use crate::{config_file::parse, error::ConfigError};

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
        let config = parse(text).unwrap();
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_debug_snapshot!(config);
        });
    }

    #[test]
    fn invalid_toml_becomes_a_typed_parse_fault() {
        let parsed = parse("volume = \"not-a-number\"");
        assert!(matches!(parsed, Err(ConfigError::Parse { .. })));
    }

    #[rstest]
    #[case::an_unknown_top_level_table("unknown_table", "[nope]\nkey = 1\n")]
    #[case::an_unknown_key_in_a_known_table("unknown_key", "[audio]\nbogus = 1\n")]
    fn unknown_toml_names_the_key(#[case] name: &str, #[case] text: &str) {
        let error = parse(text).expect_err("unknown TOML must not parse");
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(error.to_string());
        });
    }

    #[test]
    fn a_configured_music_dir_is_kept_as_written() {
        let config = parse("music_dir = \"~/Music\"\n").unwrap();
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
        let config = parse(&text).unwrap();
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
            matches!(parse(&text), Err(ConfigError::Parse { .. })),
            "{spelling:?} must not parse"
        );
    }

    #[test]
    fn sleep_presets_are_read_as_whole_minutes() {
        let config = parse("[audio]\nsleep_presets = [10, 20]\n").unwrap();
        assert_eq!(
            config.audio.sleep_presets,
            vec![Duration::from_secs(600), Duration::from_secs(1200)]
        );
    }
}
