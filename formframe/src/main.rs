use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        error::MainResult,
        load::filter::is_match,
        models::{check_args, init_logging},
        prelude::{CrateResult as Result, *},
    },
    lazy_static::lazy_static,
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
            error::{CrateError, CrateResult},
        },
        tracing::{
            debug, debug_span, error, error_span as always_span, info, info_span, instrument,
            trace, trace_span, warn,
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
}

fn main() {
    init_logging();

    if let Err(e) = try_main() {
        eprintln!("Fatal: {}", e)
    }
}

fn try_main() -> MainResult<()> {
    let span = always_span!("main");
    let _grd = span.enter();

    check_args()?;
    info!("Program Args loaded");

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
