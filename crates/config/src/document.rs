use toml_edit::{DocumentMut, Item, Table};

use crate::error::Error;

pub(crate) type TomlEdit = (&'static str, &'static str, Option<Item>);

pub(crate) fn ensure_table<'doc>(
    doc: &'doc mut DocumentMut,
    key: &str,
) -> Result<&'doc mut Table, Error> {
    doc.entry(key)
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| Error::NotATable {
            key: key.to_string(),
        })
}

pub(crate) fn write_edits<const N: usize>(
    doc: &mut DocumentMut,
    fields: [TomlEdit; N],
) -> Result<(), Error> {
    fields
        .into_iter()
        .filter_map(|(table, key, item)| item.map(|item| (table, key, item)))
        .try_for_each(|(table, key, item)| {
            ensure_table(doc, table)?[key] = item;
            Ok(())
        })
}
