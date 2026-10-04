#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gain(f32);

impl Gain {
    pub const UNITY: Self = Self(1.0);
    pub(crate) const SILENCE: Self = Self(0.0);

    #[must_use]
    pub fn from_decibels(decibels: kernel::domain::Decibels) -> Self {
        Self(10f32.powf(decibels.0 / 20.0))
    }

    #[must_use]
    pub(crate) fn from_amplitude(amplitude: f32) -> Self {
        Self(amplitude)
    }

    #[must_use]
    pub fn amplitude(self) -> f32 {
        self.0
    }
}
