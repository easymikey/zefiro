use tachyonfx::Interpolation;

const MODAL_REVEAL_MS: u32 = 900;
const TOAST_SLIDE_IN_MS: u32 = 900;
const PULSE_MS: u32 = 900;
const PULSE_HALF_MS: u32 = PULSE_MS / 2;
const DELETE_BURST_MS: u32 = 900;
const THEME_WASH_MS: u32 = 400;
pub const THEME_WASH_GRADIENT_CELLS: u16 = 30;
const COVER_CROSSFADE_MS: u32 = 900;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationTimings {
    pub modal_reveal: (u32, Interpolation),
    pub(crate) modal_transition_width: f32,
    pub toast_slide_in: (u32, Interpolation),
    pub(crate) scatter_seed: u32,
    pub chip_pulse_half: (u32, Interpolation),
    pub(crate) row_flash: (u32, Interpolation),
    pub volume_pulse: (u32, Interpolation),
    pub volume_pulse_mix: f32,
    pub screen_wash: (u32, Interpolation),
    pub cover_crossfade: (u32, Interpolation),
    pub delete_burst: (u32, Interpolation),
    pub(crate) delete_force: f32,
    pub(crate) delete_force_variance: f32,
}

pub const TIMINGS: AnimationTimings = AnimationTimings {
    modal_reveal: (MODAL_REVEAL_MS, Interpolation::QuadOut),
    modal_transition_width: 12.0,
    toast_slide_in: (TOAST_SLIDE_IN_MS, Interpolation::QuadOut),
    scatter_seed: 20_260_913,
    chip_pulse_half: (PULSE_HALF_MS, Interpolation::QuadInOut),
    row_flash: (PULSE_MS, Interpolation::QuadOut),
    volume_pulse: (PULSE_MS, Interpolation::QuadOut),
    volume_pulse_mix: 0.2,
    screen_wash: (THEME_WASH_MS, Interpolation::QuadOut),
    cover_crossfade: (COVER_CROSSFADE_MS, Interpolation::QuadOut),
    delete_burst: (DELETE_BURST_MS, Interpolation::QuadOut),
    delete_force: 3.0,
    delete_force_variance: 1.0,
};
