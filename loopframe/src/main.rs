use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        models::process_single_stream,
    },
    lazy_static::lazy_static,
};

mod cli;
mod models;

lazy_static! {
    static ref ARGS: ProgramArgs = ProgramArgs::init(generate_cli());
}

fn main() {
    match process_single_stream() {
        Ok(_) => (),
        Err(e) => eprintln!("{}", e),
    }
}
