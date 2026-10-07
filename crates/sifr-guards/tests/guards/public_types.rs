use std::collections::BTreeMap;

use crate::guards::{
    lexer::{File, Item, TEST},
    support,
};

const PER_CRATE: &[&str] = &["Error"];

fn public_types(file: &File) -> Vec<(String, String)> {
    let owner = file.path.split('/').next().unwrap_or("");
    let public = file.items.iter().filter(|site| {
        site.is_type() && !site.has(TEST) && file.tx(site.at.wrapping_sub(1)) == "pub"
    });
    let at = |site: &Item| {
        let place = format!("{}:{}", file.path, site.line);
        (site.name.clone(), format!("{owner} {place}"))
    };
    public.map(at).collect()
}

fn shared_names(files: &[File]) -> Vec<String> {
    let mut places: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, place) in files.iter().flat_map(public_types) {
        places.entry(name).or_default().push(place);
    }
    let crates = |found: &Vec<String>| {
        let mut owners: Vec<&str> =
            found.iter().filter_map(|x| x.split(' ').next()).collect();
        owners.dedup();
        owners.len()
    };
    let shared = places.into_iter().filter(|(name, found)| {
        !PER_CRATE.contains(&name.as_str()) && crates(found) > 1
    });
    let line = |(name, found): (String, Vec<String>)| {
        let at: Vec<&str> = found.iter().filter_map(|x| x.split(' ').nth(1)).collect();
        format!("`{name}` is public in {}", at.join(", "))
    };
    shared.map(line).collect()
}

#[test]
fn no_public_type_name_in_two_crates() {
    let files: Vec<File> = support::source_files(&["src"])
        .iter()
        .map(|(relative, path)| File::parse(relative, &support::read(path)))
        .collect();
    support::report(
        "public type guard: one public type name lives in one crate.",
        &shared_names(&files),
    );
}

#[test]
fn shared_name_is_seen_across_crates_only() {
    let files = [
        File::parse(
            "kernel/src/a.rs",
            "pub struct Cover; pub(crate) enum Mode {}",
        ),
        File::parse("kernel/src/b.rs", "pub enum Cover {} pub enum Error {}"),
        File::parse(
            "widgets/src/c.rs",
            "pub struct Cover; struct Mode; pub enum Error {}",
        ),
    ];
    let expected = "`Cover` is public in kernel/src/a.rs:1, kernel/src/b.rs:1, \
                    widgets/src/c.rs:1";
    assert_eq!(shared_names(&files), [expected]);
}
