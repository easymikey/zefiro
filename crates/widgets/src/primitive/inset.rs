use ratatui::widgets::Padding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Inset {
    pub(crate) top: u16,
    pub(crate) left: u16,
    pub(crate) right: u16,
    pub(crate) bottom: u16,
}

impl Inset {
    #[must_use]
    pub(crate) const fn overlay() -> Self {
        Self {
            top: 0,
            left: 1,
            right: 2,
            bottom: 0,
        }
    }

    #[must_use]
    pub(crate) const fn card() -> Self {
        Self {
            top: 1,
            left: 2,
            right: 3,
            bottom: 1,
        }
    }

    #[must_use]
    pub(crate) const fn padding(self) -> Padding {
        Padding::new(self.left, self.right, self.top, self.bottom)
    }
}
