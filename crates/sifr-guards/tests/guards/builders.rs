use crate::guards::support;

const MARKERS: &[&str] = &[
    "bon::",
    "#[builder",
    "derive(Builder",
    ", Builder",
    "maybe_",
];

fn offenders(content: &str) -> Vec<(usize, &'static str)> {
    content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .flat_map(|(index, line)| {
            MARKERS
                .iter()
                .filter(move |marker| line.contains(**marker))
                .map(move |&marker| (index + 1, marker))
        })
        .collect()
}

#[test]
fn no_generated_builder_or_maybe_setter() {
    let mut violations = Vec::new();
    for (relative, path) in
        support::source_files(&["src", "tests", "benches", "examples"])
    {
        if relative.starts_with("sifr-guards/") {
            continue;
        }
        for (line, marker) in offenders(&support::read(&path)) {
            violations.push(format!("{relative}:{line}: `{marker}`"));
        }
    }
    for (crate_name, manifest) in support::manifests() {
        let depends_on_bon = ["dependencies", "dev-dependencies"]
            .iter()
            .filter_map(|section| manifest.get(section))
            .any(|table| table.get("bon").is_some());
        if depends_on_bon {
            violations.push(format!("{crate_name}/Cargo.toml: depends on `bon`"));
        }
    }
    support::report(
        "builders guard: a widget is `XWidget::new(..)` plus setters named after the field, \
         a patch is `XPatch { field: Some(v), ..XPatch::default() }`; no bon builder, no maybe_ setter.",
        &violations,
    );
}

#[test]
fn offenders_finds_each_builder_marker_and_skips_comments() {
    let content = "#[derive(Debug, bon::Builder)]\n\
                   #[builder(on(String, into))]\n\
                   let patch = Patch::builder().maybe_volume(None).build();\n// bon::Builder in a comment\n\
                   let widget = Widget::new(input).avoid(rects);\n";
    assert_eq!(
        offenders(content),
        vec![(1, "bon::"), (2, "#[builder"), (3, "maybe_")]
    );
}
