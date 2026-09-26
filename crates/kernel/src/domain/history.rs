use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
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
