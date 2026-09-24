mod catalogue;
mod play;
mod stage;
mod timings;

pub use catalogue::{
    VolumeShades,
    chip_pulse,
    delete_burst,
    favorite_pulse,
    modal_in,
    modal_out,
    row_flash,
    screen_wash,
    toast_burst,
    toast_slide_in,
    volume_pulse,
    wash_reveal,
};
pub use stage::{AnimationStage, Backdrop};
pub use timings::AnimationTimings;
