use std::{collections::HashMap, fmt, path::PathBuf};

use kernel::domain::{self, Action, KeyContext};
use serde::{
    Deserialize,
    de::{Deserializer, MapAccess, Visitor, value::MapAccessDeserializer},
};
use strum::IntoEnumIterator;

use crate::{config_file::parse_config, error::ConfigError};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryTable {
    chord: String,
    context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyBindingFile {
    chord: String,
    context: KeyContext,
}

struct EntrySpelling;

impl<'de> Visitor<'de> for EntrySpelling {
    type Value = KeyBindingFile;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a chord string, or a table of `chord` and `context`")
    }

    fn visit_str<E: serde::de::Error>(self, spelling: &str) -> Result<Self::Value, E> {
        Ok(KeyBindingFile {
            chord: spelling.to_string(),
            context: KeyContext::default(),
        })
    }

    fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        let table = EntryTable::deserialize(MapAccessDeserializer::new(map))?;
        let context = match table.context {
            Some(spelling) => spelling
                .parse::<KeyContext>()
                .map_err(serde::de::Error::custom)?,
            None => KeyContext::default(),
        };
        Ok(KeyBindingFile {
            chord: table.chord,
            context,
        })
    }
}

impl<'de> Deserialize<'de> for KeyBindingFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(EntrySpelling)
    }
}

type RawKeymap = HashMap<String, KeyBindingFile>;

#[derive(Clone, Default, PartialEq, Deserialize)]
#[serde(try_from = "RawKeymap")]
pub struct KeymapFile(HashMap<Action, KeyBindingFile>);

impl TryFrom<RawKeymap> for KeymapFile {
    type Error = strum::ParseError;

    fn try_from(raw: RawKeymap) -> Result<Self, Self::Error> {
        raw.into_iter()
            .map(|(name, binding)| Ok((name.parse::<Action>()?, binding)))
            .collect::<Result<HashMap<Action, KeyBindingFile>, Self::Error>>()
            .map(Self)
    }
}

impl fmt::Debug for KeymapFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_map()
            .entries(Action::iter().filter_map(|action| {
                let spelling: &'static str = action.into();
                Some((spelling, self.0.get(&action)?))
            }))
            .finish()
    }
}

impl From<KeymapFile> for domain::KeymapOverrides {
    fn from(file: KeymapFile) -> Self {
        file.0
            .into_iter()
            .map(|(action, binding)| {
                (
                    action,
                    domain::KeyOverride {
                        chord: binding.chord,
                        context: binding.context,
                    },
                )
            })
            .collect()
    }
}

#[must_use]
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedKeymap {
    pub keymap: domain::KeymapOverrides,
    pub music_dir: Option<PathBuf>,
}

pub fn parse_keymap(text: &str) -> Result<ParsedKeymap, ConfigError> {
    parse_config(text).map(|config| ParsedKeymap {
        keymap: config.keymap.into(),
        music_dir: config.music_dir,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::domain::{Action, KeyOverride, KeymapOverrides};

    use crate::{
        error::ConfigError,
        keymap::{ParsedKeymap, parse_keymap},
    };

    #[test]
    fn keymap_fault_names_the_file_and_the_line() {
        let Err(fault) = parse_keymap("[keymap]\nnext = \"x\"\n[keymap]\n") else {
            panic!("a broken config file must not parse");
        };
        let text = fault.to_string();
        assert_eq!(text.lines().nth(1), Some("config.toml:3"), "was {text:?}");
    }

    #[test]
    fn a_config_file_yields_its_keymap_and_its_root_and_ignores_other_tables() {
        let parsed = parse_keymap(
            "music_dir = \"/tmp\"\ntheme = \"dark\"\n\n[audio]\ncrossfade = \"3s\"\n\n[keymap]\nnext = \"x\"\n",
        );
        assert_eq!(
            parsed.ok(),
            Some(ParsedKeymap {
                keymap: KeymapOverrides::from([(Action::Next, KeyOverride::from("x"))]),
                music_dir: Some(PathBuf::from("/tmp")),
            })
        );
    }

    #[test]
    fn a_broken_config_file_reports_a_parse_fault() {
        assert!(matches!(
            parse_keymap("[keymap\nnot toml"),
            Err(ConfigError::Parse { .. })
        ));
    }
}
