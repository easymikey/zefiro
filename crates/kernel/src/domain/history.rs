use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    #[serde(rename = "ts")]
    pub at: i64,
}

#[derive(Debug, Clone)]
pub struct History {
    pub view: Vec<HistoryEntry>,
    pub view_cap: usize,
}

impl Default for History {
    fn default() -> Self {
        Self {
            view: Vec::new(),
            view_cap: 200,
        }
    }
}
