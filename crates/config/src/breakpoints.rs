use serde::Deserialize;

use crate::appearance::LayoutMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    pub full_min_width: u16,
    pub full_min_height: u16,
    pub compact_min_width: u16,
    pub compact_min_height: u16,
    pub min_columns: u16,
    pub min_rows: u16,
    pub mode: LayoutMode,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            full_min_width: 60,
            full_min_height: 19,
            compact_min_width: 30,
            compact_min_height: 13,
            min_columns: 48,
            min_rows: 16,
            mode: LayoutMode::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::breakpoints::LayoutConfig;

    #[test]
    fn the_stock_breakpoints_are_the_documented_defaults() {
        insta::assert_debug_snapshot!(LayoutConfig::default());
    }
}
