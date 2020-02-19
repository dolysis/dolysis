use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        error::MainResult,
        load::filter::is_match,
        models::{check_args, init_logging, tcp::listener},
        prelude::{CrateResult as Result, *},
    },
    lazy_static::lazy_static,
    tracing_futures::Instrument,
};

mod cli;
mod error;
mod graph;
mod load;
mod models;

mod prelude {
    pub use {
        crate::{
            cli, enter,
            error::{CrateError, CrateResult, LogError},
        },
        tracing::{
            debug, debug_span, error, error_span as always_span, info, info_span, instrument,
            trace, trace_span, warn, Level,
        },
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

    //try_main().map_err(|e| e.into())

    let data = read_from(cli!().get_input())?;
    cli!().get_filter().access_set(|arena, set| {
        println!("Using '{}' as the data...", &data);
        for (name, root) in set.iter() {
            println!("Accessing regex set for: '{}'...", name);
            let b = arena
                .get(*root)
                .unwrap()
                .traverse_with(&|a, d, c| is_match(a, d, c, &data), arena);
            println!("Is the data a match for '{}'? | {}", name, b);
        }
    });

    Ok(())
}

#[tokio::main]
async fn try_main() -> Result<()> {
    let addr = cli!().bind_addr();
    listener(addr)
        .instrument(always_span!("listener.tcp", bind = addr))
        .await
}

fn read_from(source: Option<&std::path::Path>) -> Result<String> {
    use std::io::Read;
    let mut s = String::new();

    match source {
        None => std::io::stdin()
            .read_to_string(&mut s)
            .map(|_| s)
            .map_err(|e| e.into()),
        Some(p) => std::fs::File::open(p)
            .and_then(|mut f| f.read_to_string(&mut s))
            .map(|_| s)
            .map_err(|e| e.into()),
    }
}
