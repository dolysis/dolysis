use {
    crate::{
        models::WriteChannel,
        output::{AsMapSerialize, Directive, Item, OutputContext},
        prelude::*,
    },
    bstr::io::BufReadExt,
    chrono::Utc,
    crossbeam_channel::Sender,
    futures::{channel::mpsc::Sender as AsyncSender, executor::block_on, prelude::*},
    serde_interface::{KindMarker, RecordInterface, RecordKind},
    std::{
        io,
        path::Path,
        process::{Child, Command, Stdio},
    },
};

/// Execute a path and return a process handle that has stdin closed
/// and stdout / stderr stored for use
pub fn spawn_process<T>(path: T) -> Result<Child>
where
    T: AsRef<Path>,
{
    Command::new(path.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map(|mut child| {
            // Ensure stdin is closed
            drop(child.stdin.take());
            child
        })
        .map_err(|e| e.into())
}

/// Macro function for processing Child stdout and stderr.
/// Attempts to parallelize output processing, if the underlying thread
/// pool is not currently full
pub fn process_child(
    mut handle: Child,
    context: &OutputContext,
    tx_write: &mut AsyncSender<WriteChannel>,
    tx_child: &mut Sender<Child>,
) -> Result<()> {
    trace!("Processing child {}", handle.id());

    let mut body = || -> Result<()> {
        let mut sink =
            RecordInterface::new_sink(tx_write.clone().sink_map_err(|e| CrateError::from(e)));

        block_on(sink.send(RecordKind::new(
            KindMarker::Header,
            AsMapSerialize::new(context.stream(&[
                Item::Tag(Directive::Begin),
                Item::Time(Utc::now().timestamp_nanos()),
            ])),
        )))?;
        trace!("Sent opening header");

        match (handle.stdout.take(), handle.stderr.take()) {
            // Attempt to parallelize output streams, if capacity in worker pool exists
            (Some(ref mut stdout), Some(ref mut stderr)) => {
                let results = rayon::join(
                    || process_child_output(Directive::Stdout, &context, stdout, tx_write.clone()),
                    || process_child_output(Directive::Stderr, &context, stderr, tx_write.clone()),
                );
                results.0.and(results.1)?
            }
            (Some(ref mut stdout), None) => {
                process_child_output(Directive::Stdout, &context, stdout, tx_write.clone())?
            }
            (None, Some(ref mut stderr)) => {
                process_child_output(Directive::Stderr, &context, stderr, tx_write.clone())?
            }
            (None, None) => (),
        }

        block_on(sink.send(RecordKind::new(
            KindMarker::Header,
            AsMapSerialize::new(context.stream(&[
                Item::Tag(Directive::End),
                Item::Time(Utc::now().timestamp_nanos()),
            ])),
        )))?;
        trace!("Sent closing header");

        Ok(())
    };
    match body() {
        defer => tx_child
            .send(handle)
            .map_err(|e| e.into())
            .and_then(|_| defer)
            .log(Level::ERROR),
    }
}

/// Serializes a child's output and sends it to
/// the writer thread, with no intermediate allocations
fn process_child_output<R>(
    directive: Directive,
    context: &OutputContext,
    read: R,
    tx_write: AsyncSender<WriteChannel>,
) -> Result<()>
where
    R: io::Read + Send,
{
    enter!(
        span,
        always_span!("child.stream", kind = %directive.span_display())
    );
    trace!("Processing child output stream");

    let mut log_t_lines = 0u64;
    let mut log_t_bytes = 0u64;

    let buffer = io::BufReader::new(read);
    let mut sink =
        RecordInterface::new_sink(tx_write.clone().sink_map_err(|e| CrateError::from(e)));

    buffer
        .for_byte_line(|line| {
            block_on(sink.send(RecordKind::new(
                KindMarker::Data,
                AsMapSerialize::new(context.stream(&[
                    Item::Tag(directive),
                    Item::Time(Utc::now().timestamp_nanos()),
                    Item::Data(line),
                ])),
            )))
            //Ugly workaround for closure's io::Error requirement,
            //Round trips from our local error into io::Error and back
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            .map(|o| {
                log_t_lines += 1;
                log_t_bytes += line.len() as u64;
                o
            })
            .and(Ok(true))
        })
        .map(|_| {
            if log_t_bytes > 0 {
                debug!(
                    lines = log_t_lines,
                    bytes = log_t_bytes,
                    "Finished child stream"
                )
            }
        })
        .map_err(|e| e.into())
}
