use std::time::Duration;

use crate::domain::{
    chord::ChordPrefix,
    config::ConfigErrors,
    cursor::Cursor,
    geometry::{Cells, Pixels},
    index::ViewIndex,
    keymap::Keymap,
    library::SortKey,
    overlay::Overlay,
    time::Moment,
    toast::Toast,
};

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub overlay: Option<Overlay>,
    pub browse: Browse,
    pub chord_prefix: Option<ChordPrefix>,
    pub toasts: Vec<Toast>,
    pub(crate) clock: Moment,
    pub keymap: Keymap,
    pub visible_rows: Cells,
    pub cover_side: Option<Pixels>,
    pub played_for: Duration,
    pub(crate) config_errors: ConfigErrors,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Browse {
    pub cursor: Cursor,
    pub sort: SortKey,
}

impl Browse {
    #[must_use]
    pub fn selected(&self) -> ViewIndex {
        ViewIndex::new(self.cursor.index())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePhase {
    Prompt,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveLine {
    pub text: String,
    pub phase: SavePhase,
}
