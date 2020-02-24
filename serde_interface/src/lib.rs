mod error;
mod markers;
mod spec;
mod tokio_cbor;
mod traits;

pub use crate::{
    error::CrateError as InterfaceError,
    markers::{DataContext, KindMarker, TagMarker},
    spec::{Record, RecordKind},
    tokio_cbor::{cbor_write, Cbor, RecordSink, SymmetricalCbor},
    traits::{Marker, Repr},
};
