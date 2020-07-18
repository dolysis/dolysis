use {
    crate::{
        models::WriteChannel,
        output::{DataBuilder, Directive, HeaderBuilder, OutputContext},
        prelude::*,
    },
    bstr::io::BufReadExt,
    chrono::Utc,
    crossbeam_channel::Sender,
    futures::{channel::mpsc::Sender as AsyncSender, executor::block_on, prelude::*},
    lib_transport::{DataContext, RecordInterface},
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
        let mut sink = RecordInterface::new_sink(tx_write.clone().sink_map_err(CrateError::from));

        block_on(sink.send(header(context, Directive::Start).done_unchecked()))?;
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

        block_on(sink.send(header(context, Directive::End).done_unchecked()))?;
        trace!("Sent closing header");

        Ok(())
    };
    let defer = body();

    tx_child
        .send(handle)
        .map_err(|e| e.into())
        .and_then(|_| defer)
        .log(Level::ERROR)
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

    let mut lines = 0u64;
    let mut bytes = 0u64;

    let buffer = io::BufReader::new(read);
    let mut sink = RecordInterface::new_sink(tx_write.sink_map_err(CrateError::from));

    buffer
        .for_byte_line(|line| {
            let utf8 = String::from_utf8_lossy(line);

            block_on(sink.send(data(context, directive, &utf8).done_unchecked()))
                //Ugly workaround for closure's io::Error requirement,
                //Round trips from our local error into io::Error and back
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                .map(|o| {
                    lines += 1;
                    bytes += line.len() as u64;
                    o
                })
                .and(Ok(true))
        })
        .map(|_| {
            if bytes > 0 {
                debug!(lines, bytes, "Finished child stream")
            }
        })
        .map_err(|e| e.into())
}

fn header<T>(cxt: &OutputContext, tag: T) -> HeaderBuilder<'_>
where
    T: Into<DataContext>,
{
    HeaderBuilder::new(Some(cxt)).map(|this| {
        this.and(|this| this.time(now())).and(|this| this.tag(tag));
    })
}

fn data<'ctx, 'out, T>(cxt: &'ctx OutputContext, tag: T, data: &'out str) -> DataBuilder<'ctx, 'out>
where
    T: Into<DataContext>,
{
    DataBuilder::new(Some(cxt)).map(|this| {
        this.and(|this| this.time(now()))
            .and(|this| this.tag(tag))
            .and(|this| this.data(data));
    })
}

#[inline]
fn now() -> i64 {
    Utc::now().timestamp_nanos()
}
