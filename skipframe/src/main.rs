use {
    crate::{
        cli::{generate_cli, ProgramArgs},
        models::{get_executables_sorted, process_list, worker_wait, write_select, WriteChannel},
        prelude::*,
    },
    crossbeam_channel::bounded,
    futures::channel::mpsc::{channel as async_bounded, Receiver},
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
    let (tx_write, rx_write) = async_bounded::<WriteChannel>(1024);
    let (tx_child, rx_child) = bounded::<std::process::Child>(1024);

    let child = worker_wait(rx_child);
    tokio_entry(rx_write).unwrap();

    process_list(
        || get_executables_sorted(ARGS.exec_root()),
        tx_write,
        tx_child,
    );

    child.join().unwrap().unwrap();
}

#[tokio::main]
async fn tokio_entry(rx_write: Receiver<WriteChannel>) -> Result<()> {
    write_select(rx_write).await
}
