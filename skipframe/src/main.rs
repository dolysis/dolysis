use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        models::{get_executables_sorted, process_list, worker_wait, worker_write, WriteChannel},
    },
    crossbeam_channel::bounded,
    lazy_static::lazy_static,
};

mod cli;
mod compare;
mod error;
mod models;
mod output;
mod process;

mod prelude {
    pub use crate::error::{Error as CrateError, Result};
}

lazy_static! {
    static ref ARGS: ProgramArgs = ProgramArgs::init(generate_cli());
}

fn main() {
    let (tx_write, rx_write) = bounded::<WriteChannel>(1024);
    let (tx_child, rx_child) = bounded::<std::process::Child>(1024);

    let child = worker_wait(rx_child);
    let wrt = worker_write(rx_write);

    process_list(
        || get_executables_sorted(ARGS.exec_root()),
        tx_write,
        tx_child,
    );

    child.join();
    wrt.join();
}
