/// Provides a simple abstraction for de/serializing
/// objects based on a foreign object
pub trait Marker {
    type Marker: Clone + Copy + Sync + Send;

    fn as_marker(&self) -> Self::Marker;
}

/// Useful for enums that are [repr(u_)]
pub trait Repr: Marker {
    fn repr_u8(&self) -> u32;
}
