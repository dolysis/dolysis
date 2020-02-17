use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        load::filter::{is_match, FilterSet},
        prelude::*,
    },
    lazy_static::lazy_static,
    std::sync::Arc,
};

mod cli;
mod error;
mod graph;
mod load;

mod prelude {
    pub use crate::error::{CrateError, Result};
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

fn main() {
    if let Err(e) = try_main() {
        eprintln!("Fatal: {}", e)
    }
}

fn try_main() -> Result<()> {
    check_args()?;

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

fn check_args() -> Result<()> {
    let args = ARGS.as_ref();
    match args {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
