use std::path::PathBuf;

use crate::domain::UnixSeconds;

pub const HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub at: UnixSeconds,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    pub view: Vec<HistoryEntry>,
}
