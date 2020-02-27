use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        models::{init_logging, process_single_stream},
        prelude::*,
    },
    lazy_static::lazy_static,
};

mod cli;
mod models;
mod prelude {
    pub use {
        crate::enter,
        tracing::{
            debug, debug_span, error, error_span as always_span, field::Empty, info, info_span,
            instrument, trace, trace_span, warn, Level,
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
    if let Err(e) = tokio_main() {
        error!(fatal = %e);
    }
}

#[tokio::main]
async fn tokio_main() -> Result<(), std::io::Error> {
    process_single_stream()
        .instrument(always_span!("tokio"))
        .await
}
