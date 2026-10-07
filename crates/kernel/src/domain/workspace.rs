use crate::{
    domain::{
        chord::ChordPrefix,
        config::ConfigErrors,
        cursor::Cursor,
        geometry::{Cells, Pixels},
        index::ViewIndex,
        library::SortKey,
        overlay::Overlay,
        time::Moment,
        toast::Toast,
    },
    update::keymap::bindings::Keymap,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Workspace {
    pub overlay: Option<Overlay>,
    pub browse: Browse,
    pub chord_prefix: Option<ChordPrefix>,
    pub toasts: Vec<Toast>,
    pub(crate) clock: Moment,
    pub keymap: Keymap,
    pub visible_rows: Cells,
    pub cover_side: Option<Pixels>,
    pub(crate) config_errors: ConfigErrors,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Browse {
    pub cursor: Cursor,
    pub sort_key: SortKey,
}

impl Browse {
    #[must_use]
    pub fn selected(&self) -> ViewIndex {
        ViewIndex::new(self.cursor.index())
    }
}
