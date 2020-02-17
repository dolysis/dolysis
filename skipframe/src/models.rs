use {
    crate::{
        compare::{by_priority, Priority},
        output::OutputContext,
        prelude::*,
        process::{process_child, serialize_error, spawn_process},
        ARGS,
    },
    crossbeam_channel::{unbounded, Receiver, Sender},
    rayon::{iter::ParallelBridge, prelude::*},
    serde_interface::Record,
    std::{
        convert::TryFrom,
        io,
        net::{Shutdown, TcpStream},
        os::unix::fs::PermissionsExt,
        path::Path,
        process::Child,
        thread,
    },
    walkdir::{DirEntry, WalkDir},
};

/// Alias for the type sent to the writer thread
pub type WriteChannel = Option<Vec<u8>>;

/// Responsible for running, processing and serializing the output of, the executable paths
/// passed in. This function assumes that the given iterator's output is sorted by Priority,
/// _and is already sorted_. It will attempt to run anything of the same Priority in parallel
/// given there are system resources to do so. After serializing it sends the byte buffer to
/// a channel whose receiver is responsible for writing the data out
pub fn process_list<F, I>(f: F, writer_tx: Sender<WriteChannel>, child_tx: Sender<Child>)
where
    F: FnOnce() -> I,
    I: Iterator<Item = Result<(Priority, DirEntry)>> + Send,
{
    let (fctl_tx, fctl_rx): (Sender<()>, Receiver<()>) = unbounded();

    f().scan((None, 0u64), |state, result| -> Option<Result<DirEntry>> {
        let (prev, count) = state;
        match result {
            Ok((priority, entry)) => {
                if priority == *prev.get_or_insert_with(|| priority) {
                    *count += 1;
                    Some(Ok(entry))
                } else {
                    *prev = Some(priority);
                    // Note that this iter can block
                    for _ in fctl_rx.iter() {
                        if *count != 0 {
                            *count -= 1;
                        }
                        if *count == 0 {
                            return Some(Ok(entry));
                        }
                    }
                    assert!(*count == 0);
                    Some(Ok(entry))
                }
            }
            Err(e) => Some(Err(e)),
        }
    })
    .par_bridge()
    .map(|result| {
        result.map(|entry| {
            let mut bld = OutputContext::new();
            bld.insert_id(entry.path().file_name().unwrap().to_str().unwrap());
            (entry, bld)
        })
    })
    .for_each_with(
        (fctl_tx, writer_tx.clone(), child_tx),
        |(fctl, writer, child), result| {
            result
                .and_then(|(entry, mut bld)| {
                    spawn_process(entry.path()).and_then(|handle| {
                        bld.insert_pid(handle.id());
                        process_child(handle, &bld, writer, child)
                    })
                })
                .unwrap_or_else(|e| serialize_error(e, writer));

            fctl.send(());
        },
    );

    writer_tx.send(None);
}

/// Returns a iterator of Prioritized DirEntries that are guaranteed to be executable and NOT a directory.
/// In practice this is equivalent to a executable file, however evil use of symlinks could cause a non-file descriptor
/// to pass through this filter.
// I haven't bothered to fix this vulnerability because:
// A. It would require multiple calls to stat
// B. It is incredibly unlikely a user will stumble into a pathological case by accident
pub fn get_executables_sorted<T>(dir_root: T) -> impl Iterator<Item = Result<(Priority, DirEntry)>>
where
    T: AsRef<Path>,
{
    WalkDir::new(dir_root)
        .sort_by(|a, b| by_priority(a, b))
        .into_iter()
        .filter_entry(|entry| {
            entry.file_type().is_dir()
                || (entry.file_type().is_file() && is_executable(entry).unwrap_or(false))
        })
        .filter(|res| {
            res.as_ref()
                .map(|e| !e.file_type().is_dir())
                // Pass errors through
                .unwrap_or(true)
        })
        .map(|res| {
            res.map_err(|e| e.into())
                .and_then(|entry| Priority::try_from(&entry).map(|priority| (priority, entry)))
        })
}

/// Spawns a worker that handles all outbound writing done by this program
pub fn worker_write(rx_writer: Receiver<WriteChannel>) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || write_select(rx_writer))
}

/// Receives all child processes that the main program is finished with and waits
/// them. This is required on some architectures for the OS to release system resources.
/// Waiting on a separate worker allows the rayon pool (which wants to be CPU bound)
/// to avoid blocking
pub fn worker_wait(rx_child: Receiver<Child>) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        for mut child in rx_child.iter() {
            child.wait();
        }

        Ok(())
    })
}

/// Selects the output channel based on user input
pub fn write_select(rx_writer: Receiver<WriteChannel>) -> Result<()> {
    match (ARGS.con_socket(), ARGS.con_tcp(), ARGS.con_stdout()) {
        (Some(socket), _, _) => {
            if cfg!(target_family = "unix") {
                use std::os::unix::net::UnixStream;
                let mut socket = UnixStream::connect(socket)?;
                write_cbor(rx_writer, &mut socket)?;
                socket.shutdown(Shutdown::Write)?;
                Ok(())
            } else {
                // Should not be possible to hit this path as con_socket() should always return None on
                // non-unix systems
                panic!("Attempted to use unix specific socket implementation on a non unix system")
            }
        }
        (_, Some(addr), _) => {
            let mut tcp = TcpStream::connect(addr)?;
            write_cbor(rx_writer, &mut tcp)?;
            tcp.shutdown(Shutdown::Write)?;
            Ok(())
        }
        (_, _, Some(_)) => write_debug(rx_writer),
        _ => unreachable!(),
    }
}

/// Core functionality of the writer worker
fn write_cbor<W>(rx_writer: Receiver<WriteChannel>, writer: W) -> Result<()>
where
    W: io::Write,
{
    use io::Write;
    let mut buffer = io::BufWriter::new(writer);

    serde_cbor::to_writer(&mut buffer, &Record::StreamStart)?;
    for opt in rx_writer.iter() {
        match opt {
            Some(cbor) => {
                &mut buffer.write(&cbor)?;
            }
            None => break,
        }
    }
    serde_cbor::to_writer(&mut buffer, &Record::StreamEnd)?;

    buffer.flush()?;
    Ok(())
}

/// Prints to stdout, but as rust's Debug impl of the records not cbor. Should mostly be used
/// for debugging purposes
fn write_debug(rx_writer: Receiver<WriteChannel>) -> Result<()> {
    use io::Write;
    let mut buffer = io::BufWriter::new(io::stdout());

    // Yes it is wasteful to serialize and then deserialize (and allocate!) for a single item;
    // but this function will mainly be used for debugging output, and doing the whole process
    // reduces the chances of bugs only showing up in the "real function" or vice versa.
    macro_rules! gen_record {
        ($rcd:expr) => {
            match serde_cbor::to_vec(&$rcd)
                .and_then(|cbor| serde_cbor::from_slice::<serde_interface::Record>(&cbor))
            {
                Ok(record) => writeln!(&mut buffer, "{:?}", record)?,
                Err(e) => writeln!(io::stderr(), "{}", e)?,
            }
        };
    }

    gen_record!(Record::StreamStart);
    for opt in rx_writer.iter() {
        match opt.map(|cbor| serde_cbor::from_slice::<serde_interface::Record>(&cbor)) {
            Some(Ok(record)) => writeln!(&mut buffer, "{:?}", record)?,
            Some(Err(e)) => writeln!(io::stderr(), "{}", e)?,
            None => break,
        }
    }
    gen_record!(Record::StreamEnd);

    buffer.flush()?;
    Ok(())
}

/// Unix specific, checks file mode bits for executable status
// TODO: Find a way to determine if a file is executable on non-unix systems
fn is_executable(entry: &DirEntry) -> Result<bool> {
    entry
        .metadata()
        .map(|meta| mode_exec(meta.permissions().mode()))
        .map_err(|e| e.into())
}

/// AND's exec bits
fn mode_exec(mode: u32) -> bool {
    mode & 0o111 != 0
}
