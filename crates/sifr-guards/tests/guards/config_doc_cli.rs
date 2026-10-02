// GUARD: the `defaults:config` doc block parses into `Config::default()`.

use config::{ConfigToml, parse_config};

use crate::guards::{fault::TestFault, support};

const START_MARKER: &str = "<!-- defaults:config -->";
const END_MARKER: &str = "<!-- /defaults:config -->";

fn extract_config_defaults_block(doc: &str) -> Result<&str, TestFault> {
    let after_start = doc
        .split_once(START_MARKER)
        .ok_or_else(|| {
            TestFault::missing(
                "docs/config.md must contain a <!-- defaults:config --> marker",
            )
        })?
        .1;
    let block = after_start
        .split_once(END_MARKER)
        .ok_or_else(|| {
            TestFault::missing(
                "docs/config.md must contain a matching <!-- /defaults:config --> marker",
            )
        })?
        .0;
    let after_fence_open = block
        .split_once("```toml")
        .ok_or_else(|| {
            TestFault::missing(
                "the defaults:config block must open with a ```toml fence",
            )
        })?
        .1;
    Ok(after_fence_open
        .rsplit_once("```")
        .ok_or_else(|| {
            TestFault::missing("the defaults:config block must close with a ``` fence")
        })?
        .0)
}

#[test]
fn config_defaults_block_matches_config_default() -> Result<(), TestFault> {
    let doc_path = support::workspace_root().join("docs").join("config.md");
    let doc = std::fs::read_to_string(&doc_path)?;

    let toml_text = extract_config_defaults_block(&doc)?;
    let parsed = parse_config(toml_text)?;

    assert_eq!(
        parsed,
        ConfigToml::default(),
        "docs/config.md's `config.toml` defaults block has drifted from \
         Config::default() — update the TOML between the <!-- defaults:config --> \
         / <!-- /defaults:config --> markers in docs/config.md to match the new \
         default (see that file's \"Regenerating the default blocks\" section)"
    );
    Ok(())
}
