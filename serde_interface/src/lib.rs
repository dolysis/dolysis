mod error;
mod markers;
mod spec;
mod tokio_cbor;
mod traits;

pub use crate::{
    error::CrateError as InterfaceError,
    markers::{DataContext, KindMarker, TagMarker},
    spec::{Record, RecordKind},
    tokio_cbor::{Cbor, RecordFrame, RecordInterface, SymmetricalCbor},
    traits::{Marker, Repr},
};
