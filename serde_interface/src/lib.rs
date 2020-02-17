mod error;
mod markers;
mod spec;
mod traits;

pub use crate::{
    error::CrateError as InterfaceError,
    markers::{DataContext, KindMarker, TagMarker},
    spec::{Record, RecordKind},
    traits::{Marker, Repr},
};
