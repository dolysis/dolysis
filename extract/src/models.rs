use {
    crate::{
        compare::{by_priority, Priority},
        output::OutputContext,
        prelude::*,
        process::{process_child, spawn_process},
        ARGS,
    },
    bytes::Bytes,
    crossbeam_channel::{unbounded, Receiver, Sender},
    futures::{
        channel::mpsc::{Receiver as AsyncReceiver, Sender as AsyncSender},
        io::Cursor,
        prelude::*,
    },
    lib_transport::{Record, RecordFrame, RecordInterface},
    rayon::{iter::ParallelBridge, prelude::*},
    std::{
        convert::TryFrom, fmt, marker::Unpin, os::unix::fs::PermissionsExt, path::Path,
        process::Child, thread,
    },
    tokio::net::TcpStream,
    tokio_util::compat::FuturesAsyncReadCompatExt,
    tracing_subscriber::{EnvFilter, FmtSubscriber},
    walkdir::{DirEntry, WalkDir},
};

/// Alias for the type sent to the writer thread
pub type WriteChannel = Bytes;

/// Responsible for running, processing and serializing the output of, the executable paths
/// passed in. This function assumes that the given iterator's output is sorted by Priority,
/// _and is already sorted_. It will attempt to run anything of the same Priority in parallel
/// given there are system resources to do so. After serializing it sends the byte buffer to
/// a channel whose receiver is responsible for writing the data out
#[instrument(skip(f, writer_tx, child_tx))]
pub fn process_list<F, I>(f: F, writer_tx: AsyncSender<WriteChannel>, child_tx: Sender<Child>)
where
    F: FnOnce() -> I,
    I: Iterator<Item = Result<(Priority, DirEntry)>> + Send,
{
    let (fctl_tx, fctl_rx): (Sender<()>, Receiver<()>) = unbounded();
    let mut record_sink =
        RecordInterface::new_sink(writer_tx.clone().sink_map_err(CrateError::from));
    futures::executor::block_on(record_sink.send(Record::StreamStart)).unwrap();

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
            bld.insert_version(1);
            (entry, bld)
        })
    })
    .for_each_with(
        (fctl_tx, writer_tx.clone(), child_tx),
        |(fctl, writer, child), result| {
            enter!(always_span!("rayon"));
            result
                .and_then(|(entry, mut bld)| {
                    spawn_process(entry.path()).and_then(|handle| {
                        enter!(always_span!("child.process", path = %entry.path().display(), pid = handle.id()));
                        bld.insert_pid(handle.id());
                        process_child(handle, &bld, writer, child)
                    })
                })
                .unwrap_or_else(|e| {e.log(Level::ERROR);});

            fctl.send(())
                .expect("Flow control rx cannot close before the tx");
        },
    );
    futures::executor::block_on(record_sink.send(Record::StreamEnd)).unwrap();

    drop(writer_tx);
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

/// Initialize the global logger. This function must be called before ARGS is initialized,
/// otherwise logs generated during CLI parsing will be silently ignored
pub fn init_logging() {
    let root_subscriber = FmtSubscriber::builder()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::default().add_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        }))
        .with_filter_reloading()
        .finish();
    tracing::subscriber::set_global_default(root_subscriber).expect("Failed to init logging");
    info!("<== Logs Start ==>")
}

/// Receives all child processes that the main program is finished with and waits
/// them. This is required on some architectures for the OS to release system resources.
/// Waiting on a separate worker allows the rayon pool (which wants to be CPU bound)
/// to avoid blocking
pub fn worker_wait(rx_child: Receiver<Child>) -> thread::JoinHandle<Result<()>> {
    thread::spawn(move || {
        enter!(always_span!("child.cemetary"));
        for mut child in rx_child.iter() {
            let id = child.id();
            match child.wait() {
                Ok(status) if !status.success() => warn!(pid = id, %status),
                Ok(status) => debug!(pid = id, %status),
                Err(e) => {
                    CrateError::from(e).log(Level::WARN);
                }
            }
        }

        Ok(())
    })
}

/// Selects the output channel based on user input
pub async fn write_select(rx_writer: AsyncReceiver<WriteChannel>) -> Result<()> {
    match (ARGS.con_socket(), ARGS.con_tcp(), ARGS.con_stdout()) {
        (Some(socket), _, _) => {
            if cfg!(target_family = "unix") {
                use tokio::net::UnixStream;
                async {
                    debug!("Attempting socket connection...");
                    UnixStream::connect(socket)
                        .map_err(CrateError::from)
                        .inspect(|res| match res {
                            Ok(_) => info!("Connection established"),
                            Err(ref e) => e.ref_log(Level::ERROR),
                        })
                        .and_then(|socket| write_cbor(rx_writer, socket))
                        .await
                }
                .instrument(always_span!("unixstream", socket = %socket.display()))
                .await
            } else {
                // Should not be possible to hit this path as con_socket() should always return None on
                // non-unix systems
                panic!("Attempted to use unix specific socket implementation on a non unix system")
            }
        }
        (_, Some(addr), _) => {
            async {
                TcpStream::connect(addr)
                    .map_err(CrateError::from)
                    .inspect(|res| match res {
                        Ok(_) => info!("Connection established"),
                        Err(ref e) => e.ref_log(Level::ERROR),
                    })
                    .and_then(|socket| write_cbor(rx_writer, socket))
                    .await
            }
            .instrument(always_span!("tcp", bind = %addr.0, port = addr.1))
            .await
        }
        (_, _, Some(_)) => {
            write_debug(rx_writer)
                .instrument(always_span!("debug", socket = "stdout"))
                .await
        }
        _ => unreachable!(),
    }
}

/// Core functionality of the writer worker
async fn write_cbor<W>(rx_writer: AsyncReceiver<WriteChannel>, writer: W) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let buffer = tokio::io::BufWriter::new(writer);
    rx_writer
        .map(Ok)
        .forward(RecordFrame::write(buffer))
        .await?;

    info!("All data written successfully, closing the connection");

    Ok(())
}

/// Prints to stdout, but as rust's Debug impl of the records not cbor. Should mostly be used
/// for debugging purposes
async fn write_debug(rx_writer: AsyncReceiver<WriteChannel>) -> Result<()> {
    let mut buffer = Cursor::new(Vec::new()).compat();
    {
        let mut frame = RecordFrame::read_write(&mut buffer);

        rx_writer
            .inspect(|item| trace!("Writer received item, sized: {}", item.len()))
            .map(Ok)
            .forward(&mut frame)
            .await?;
    }
    buffer.get_mut().set_position(0);

    let mut record_stream = RecordInterface::new_stream(RecordFrame::read(&mut buffer));

    while let Some(record) = record_stream.try_next().await? {
        println!("{:?}", record)
    }

    info!("All data written successfully");

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

pub trait SpanDisplay {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn span_display(&self) -> LocalDisplay<Self>
    where
        Self: Sized,
    {
        LocalDisplay::new(self)
    }
}

pub struct LocalDisplay<'a, T> {
    owner: &'a T,
}

impl<'a, T> LocalDisplay<'a, T> {
    pub fn new(owner: &'a T) -> Self
    where
        T: SpanDisplay,
    {
        Self { owner }
    }
}

impl<'a, T> fmt::Display for LocalDisplay<'a, T>
where
    T: SpanDisplay,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.owner.span_print(f)
    }
}
