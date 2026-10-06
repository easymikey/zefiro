// GUARD: allowlist rows `rule path name..` of the conventions guard.

use crate::guards::support::Allow;

pub(crate) fn allow_rows(rule: &str) -> Vec<Allow> {
    let row = |cells: &'static str| {
        let mut cells = cells.split_whitespace();
        let (id, path) = (cells.next(), cells.next().unwrap_or(""));
        let names = cells.filter(move |_| id == Some(rule));
        names.map(move |name| Allow::new(path, name, "migrates with its crate step"))
    };
    ALLOW.lines().flat_map(row).collect()
}

const ALLOW: &str = "\
loop kernel/src/domain/player.rs AbLoop
";
