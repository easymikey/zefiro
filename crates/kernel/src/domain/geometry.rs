#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cells(pub u16);

impl Cells {
    #[must_use]
    pub fn count(self) -> usize {
        usize::from(self.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pixels(pub u32);
