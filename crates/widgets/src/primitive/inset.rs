use ratatui::widgets::Padding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Inset {
    pub top: u16,
    pub left: u16,
    pub right: u16,
    pub bottom: u16,
}

impl Default for Inset {
    fn default() -> Self {
        Self {
            top: 0,
            left: 1,
            right: 2,
            bottom: 0,
        }
    }
}

impl Inset {
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
