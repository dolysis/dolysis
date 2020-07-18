use {
    crate::traits::{Marker, Repr},
    serde_repr::{Deserialize_repr, Serialize_repr},
};

/// Marker for the keys of a serialized record, note
/// that keys should be unique per object
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
pub enum TagMarker {
    DataContext = 0,
    Version = 1,
    Time = 2,
    Id = 3,
    Pid = 4,
    Data = 5,
    Utf8Data = 6,
    Error = 7,
}

impl Marker for TagMarker {
    type Marker = TagMarker;

    fn as_marker(&self) -> Self::Marker {
        *self
    }
}

impl Repr for TagMarker {
    fn repr_u8(&self) -> u32 {
        *self as u32
    }
}

/// Marker for what kind of record the de/serializer is expecting,
/// this is required to prevent object collisions
// i.e: situations in which the deserializer cannot determine from
// the given data which record it should be deserialized as
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
pub enum KindMarker {
    StreamStart = 0,
    StreamEnd = 1,
    Header = 2,
    Data = 3,
    Log = 4,
    Error = 5,
}

impl Marker for KindMarker {
    type Marker = KindMarker;

    fn as_marker(&self) -> Self::Marker {
        *self
    }
}

impl Repr for KindMarker {
    fn repr_u8(&self) -> u32 {
        *self as u32
    }
}

/// Marker for a context field that is present in some record objects
#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
pub enum DataContext {
    Start = 0,
    Stdout = 1,
    Stderr = 2,
    End = 3,
}

impl Marker for DataContext {
    type Marker = DataContext;

    fn as_marker(&self) -> Self::Marker {
        *self
    }
}

impl Repr for DataContext {
    fn repr_u8(&self) -> u32 {
        *self as u32
    }
}
