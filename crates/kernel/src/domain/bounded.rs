pub trait Bounded: Sized {
    type Raw: PartialOrd;

    const MIN: Self::Raw;
    const MAX: Self::Raw;

    #[must_use]
    fn within_bounds(raw: Self::Raw) -> Self;

    #[must_use]
    fn clamped(raw: Self::Raw) -> Self {
        if raw < Self::MIN {
            return Self::within_bounds(Self::MIN);
        }
        if raw > Self::MAX {
            return Self::within_bounds(Self::MAX);
        }
        Self::within_bounds(raw)
    }
}
