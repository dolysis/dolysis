#![allow(clippy::match_bool)]

use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        models::{
            get_executables_sorted, init_logging, process_list, worker_wait, write_select,
            WriteChannel,
        },
        prelude::*,
    },
    crossbeam_channel::bounded,
    futures::channel::mpsc::channel as async_bounded,
    lazy_static::lazy_static,
};

mod cli;
mod compare;
mod error;
mod models;
mod output;
mod process;

mod prelude {
    pub use {
        crate::{
            enter,
            error::{CrateError, CrateResult as Result, LogError as _},
            models::SpanDisplay,
        },
        tracing::{
            debug, debug_span, error, error_span as always_span, info, info_span, instrument,
            trace, trace_span, warn, Level,
        },
        tracing_futures::Instrument as _,
    };
}

#[macro_export]
macro_rules! enter {
    ($span:expr) => {
        let span = $span;
        let _grd = span.enter();
    };
    ($var:ident, $span:expr) => {
        let $var = $span;
        let _grd = $var.enter();
    };
}

lazy_static! {
    static ref ARGS: ProgramArgs = ProgramArgs::init(generate_cli());
}

#[instrument]
fn main() {
    init_logging();
    let mut tokio = tokio::runtime::Runtime::new().unwrap();
    let (tx_write, rx_write) = async_bounded::<WriteChannel>(1024);
    let (tx_child, rx_child) = bounded::<std::process::Child>(1024);

    let child = worker_wait(rx_child);
    let fut = tokio.spawn(write_select(rx_write).instrument(always_span!("tokio")));

    process_list(
        || get_executables_sorted(ARGS.exec_root()),
        tx_write,
        tx_child,
    );
    tokio.block_on(fut).unwrap().unwrap();
    child.join().unwrap().unwrap();
}
