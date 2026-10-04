mod frame;
mod metrics;
mod place;
mod placement;
mod prompt;

pub use frame::ModalAreas;
pub(crate) use frame::{Hint, Modal, ModalBounds, ModalSize, PlacedModal};
pub(crate) use metrics::{
    COLUMN_SPACING,
    ModalRowStyle,
    QUERY_ROWS,
    SCROLL_PADDING,
    SCROLLBAR_INSET,
    modal_title,
};
pub(crate) use place::list_capacity;
pub(crate) use placement::{
    ModalBorder,
    ModalPlacement,
    OverlayContainer,
    column_width,
    indented,
    leading_cells,
};
pub use placement::{ModalScrollAreas, OverlayAreas};
pub(crate) use prompt::{PromptBody, PromptStyle, PromptWidget};
