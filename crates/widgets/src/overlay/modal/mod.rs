mod frame;
mod metrics;
mod place;
mod placement;
mod prompt;

pub use frame::ModalAreas;
pub(crate) use frame::{Hint, Modal, ModalBounds, ModalSize, PlacedModal};
pub(crate) use metrics::{ModalMetrics, ModalRowColors, modal_title};
pub(crate) use place::list_capacity;
pub(crate) use placement::{
    ModalBorder,
    ModalChrome,
    ModalPlacement,
    OverlayContainer,
    column_width,
    indented,
    leading_cells,
};
pub use placement::{ModalScrollAreas, OverlayAreas};
pub(crate) use prompt::{Prompt, PromptBody};
