#![allow(clippy::match_bool)]

use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        error::MainResult,
        models::{check_args, init_logging, tcp::listener},
        prelude::{CrateResult as Result, *},
    },
    lazy_static::lazy_static,
    tracing_futures::Instrument,
};

mod cli;
mod error;
mod load;
mod models;

mod prelude {
    pub use {
        crate::{
            cli, enter,
            error::{CrateError, CrateResult, LogError},
            models::{IdentifyFirstLast as _, ResultInspect as _, SpanDisplay as _},
        },
        tracing::{
            debug, debug_span, error, error_span as always_span, field, info, info_span,
            instrument, trace, trace_span, warn, Level,
        },
        tracing_futures::Instrument as _,
    };
}

lazy_static! {
    pub static ref ARGS: Result<ProgramArgs> = ProgramArgs::try_init(generate_cli());
}

#[macro_export]
macro_rules! cli {
    () => {{
        use crate::ARGS;
        ARGS.as_ref().unwrap()
    }};
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

fn main() -> MainResult<()> {
    init_logging();
    check_args()?;
    enter!(always_span!("main"));
    info!("Program Args loaded");

    try_main()?;

    Ok(())
}

#[tokio::main]
async fn try_main() -> Result<()> {
    let addr = cli!().bind_addr();
    listener(addr)
        .instrument(always_span!("listener.tcp", bind = addr.0, port = addr.1))
        .await
}
