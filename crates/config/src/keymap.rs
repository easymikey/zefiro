use std::{collections::HashMap, fmt};

use kernel::domain::keymap::{Action, KeyContext, KeyOverride};
use serde::{
    Deserialize,
    de::{Deserializer, MapAccess, Visitor, value::MapAccessDeserializer},
};
use strum::IntoEnumIterator;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, expecting = "a table of chord and context")]
struct TomlKeyOverrideTable {
    chord: String,
    context: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TomlKeyOverride(pub(crate) KeyOverride);

impl fmt::Debug for TomlKeyOverride {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

struct KeyOverrideVisitor;

impl<'de> Visitor<'de> for KeyOverrideVisitor {
    type Value = TomlKeyOverride;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a chord string, or a table of `chord` and `context`")
    }

    fn visit_str<E: serde::de::Error>(self, spelling: &str) -> Result<Self::Value, E> {
        Ok(TomlKeyOverride(KeyOverride::from(spelling)))
    }

    fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
        let table = TomlKeyOverrideTable::deserialize(MapAccessDeserializer::new(map))?;
        let context = match table.context {
            Some(spelling) => spelling
                .parse::<KeyContext>()
                .map_err(serde::de::Error::custom)?,
            None => KeyContext::default(),
        };
        Ok(TomlKeyOverride(KeyOverride {
            chord: table.chord,
            key_context: context,
        }))
    }
}

impl<'de> Deserialize<'de> for TomlKeyOverride {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(KeyOverrideVisitor)
    }
}

type KeymapByName = HashMap<String, TomlKeyOverride>;

#[derive(Clone, Default, PartialEq, Deserialize)]
#[serde(try_from = "KeymapByName", expecting = "a [keymap] table")]
pub(crate) struct TomlKeymap(pub(crate) HashMap<Action, TomlKeyOverride>);

impl TryFrom<KeymapByName> for TomlKeymap {
    type Error = strum::ParseError;

    fn try_from(raw: KeymapByName) -> Result<Self, Self::Error> {
        raw.into_iter()
            .map(|(name, key_override)| Ok((name.parse::<Action>()?, key_override)))
            .collect::<Result<HashMap<Action, TomlKeyOverride>, Self::Error>>()
            .map(Self)
    }
}

impl fmt::Debug for TomlKeymap {
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
