use toml_edit::{DocumentMut, Item, Table};

use crate::error::ConfigError;

pub(crate) type Field = (&'static str, &'static str, Option<Item>);

pub(crate) fn ensure_table<'doc>(
    doc: &'doc mut DocumentMut,
    key: &str,
) -> Result<&'doc mut Table, ConfigError> {
    doc.entry(key)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| ConfigError::NotATable {
            key: key.to_string(),
        })
}

pub(crate) fn write_fields<const N: usize>(
    doc: &mut DocumentMut,
    fields: [Field; N],
) -> Result<(), ConfigError> {
    fields
        .into_iter()
        .filter_map(|(table, key, item)| item.map(|item| (table, key, item)))
        .try_for_each(|(table, key, item)| {
            ensure_table(doc, table)?[key] = item;
            Ok(())
        })
}
