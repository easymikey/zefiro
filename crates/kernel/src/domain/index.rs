use std::{fmt, marker::PhantomData};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackSpace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewSpace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresetSpace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowSpace;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Index<Space>(usize, PhantomData<Space>);

pub type TrackIndex = Index<TrackSpace>;

pub type ViewIndex = Index<ViewSpace>;

pub type PresetIndex = Index<PresetSpace>;

pub type RowIndex = Index<RowSpace>;

impl<Space> Index<Space> {
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index, PhantomData)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl<Space> fmt::Debug for Index<Space> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Index({})", self.0)
    }
}

impl<Space> fmt::Display for Index<Space> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<Space> From<Index<Space>> for usize {
    fn from(position: Index<Space>) -> Self {
        position.0
    }
}
