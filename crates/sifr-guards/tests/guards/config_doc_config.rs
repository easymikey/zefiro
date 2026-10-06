use config::config_file::{TomlSettings, parse_config};

use crate::guards::{fault::GuardError, support};

const START_MARKER: &str = "<!-- defaults:config -->";
const END_MARKER: &str = "<!-- /defaults:config -->";

fn extract_config_defaults_block(doc: &str) -> Result<&str, GuardError> {
    let after_start = doc
        .split_once(START_MARKER)
        .ok_or_else(|| {
            GuardError::missing(
                "docs/config.md must contain a <!-- defaults:config --> marker",
            )
        })?
        .1;
    let block = after_start
        .split_once(END_MARKER)
        .ok_or_else(|| {
            GuardError::missing(
                "docs/config.md must contain a matching <!-- /defaults:config --> marker",
            )
        })?
        .0;
    let after_fence_open = block
        .split_once("```toml")
        .ok_or_else(|| {
            GuardError::missing(
                "the defaults:config block must open with a ```toml fence",
            )
        })?
        .1;
    Ok(after_fence_open
        .rsplit_once("```")
        .ok_or_else(|| {
            GuardError::missing("the defaults:config block must close with a ``` fence")
        })?
        .0)
}

#[test]
fn config_defaults_block_matches_config_default() -> Result<(), GuardError> {
    let doc_path = support::workspace_root().join("docs").join("config.md");
    let doc = std::fs::read_to_string(&doc_path)?;

    let toml_text = extract_config_defaults_block(&doc)?;
    let parsed = parse_config(toml_text)?;

    assert_eq!(
        parsed,
        TomlSettings::default(),
        "docs/config.md's `config.toml` defaults block has drifted from \
         TomlSettings::default() — update the TOML between the <!-- defaults:config --> \
         / <!-- /defaults:config --> markers in docs/config.md to match the new \
         default (see that file's \"Regenerating the default blocks\" section)"
    );
    Ok(())
}
