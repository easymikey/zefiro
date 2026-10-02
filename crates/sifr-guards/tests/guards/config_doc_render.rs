// GUARD: the `defaults:window` doc block parses into the window default.

use config::{AppearanceFile, parse_appearance};

use crate::guards::{fault::TestFault, support};

const START_MARKER: &str = "<!-- defaults:window -->";
const END_MARKER: &str = "<!-- /defaults:window -->";

fn extract_window_defaults_block(doc: &str) -> Result<&str, TestFault> {
    let after_start = doc
        .split_once(START_MARKER)
        .ok_or_else(|| {
            TestFault::mismatch(
                "docs/config.md must contain a <!-- defaults:window --> marker",
            )
        })?
        .1;
    let block = after_start
        .split_once(END_MARKER)
        .ok_or_else(|| {
            TestFault::mismatch(
                "docs/config.md must contain a matching <!-- /defaults:window --> marker",
            )
        })?
        .0;
    let after_fence_open = block
        .split_once("```toml")
        .ok_or_else(|| {
            TestFault::mismatch(
                "the defaults:window block must open with a ```toml fence",
            )
        })?
        .1;
    Ok(after_fence_open
        .rsplit_once("```")
        .ok_or_else(|| {
            TestFault::mismatch("the defaults:window block must close with a ``` fence")
        })?
        .0)
}

#[test]
fn window_defaults_block_matches_window_config_default() -> Result<(), TestFault> {
    let doc_path = support::workspace_root().join("docs").join("config.md");
    let doc = std::fs::read_to_string(&doc_path)?;

    let toml_text = extract_window_defaults_block(&doc)?;
    let parsed = parse_appearance(toml_text)?;

    assert_eq!(
        parsed,
        AppearanceFile::default(),
        "docs/config.md's `sifr-ui.toml` defaults block has drifted from \
         AppearanceConfig::default() — update the TOML between the <!-- defaults:window --> \
         / <!-- /defaults:window --> markers in docs/config.md to match the new \
         default (see that file's \"Regenerating the default blocks\" section)"
    );
    Ok(())
}
