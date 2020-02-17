use {
    crate::{
        models::WriteChannel,
        output::{AsMapSerialize, Directive, Item, OutputContext, RefContext},
        prelude::*,
    },
    bstr::io::BufReadExt,
    chrono::Utc,
    crossbeam_channel::Sender,
    serde_interface::{KindMarker, Marker, Record, RecordKind},
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
    tx_write: &mut Sender<WriteChannel>,
    tx_child: &mut Sender<Child>,
) -> Result<()> {
    let mut body = || -> Result<()> {
        serialize_record(
            KindMarker::Header,
            context.stream(&[
                Item::Version(1),
                Item::Tag(Directive::Begin),
                Item::Time(Utc::now().timestamp_nanos()),
            ]),
        )
        .and_then(|vec| tx_write.send(Some(vec)).map_err(|e| e.into()))?;

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

        serialize_record(
            KindMarker::Header,
            context.stream(&[
                Item::Version(1),
                Item::Tag(Directive::End),
                Item::Time(Utc::now().timestamp_nanos()),
            ]),
        )
        .and_then(|vec| tx_write.send(Some(vec)).map_err(|e| e.into()))?;

        Ok(())
    };
    match body() {
        defer => tx_child
            .send(handle)
            .map_err(|e| e.into())
            .and_then(|_| defer),
    }
}

/// Helper function for error serialization
pub fn serialize_error(err: CrateError, tx_write: &mut Sender<WriteChannel>) {
    // Better to panic here then try handling what is either an allocation failure or the writer thread disappearing
    // If this function is hit we've already experienced an error and we should hard crash rather than trying anything funny
    let vec = serde_cbor::to_vec(&Record::new_error(1, err)).unwrap();
    tx_write.send(Some(vec)).unwrap()
}

/// Serializes a child's output and sends it to
/// the writer thread, with no intermediate allocations
fn process_child_output<R>(
    directive: Directive,
    context: &OutputContext,
    read: R,
    tx_writer: Sender<WriteChannel>,
) -> Result<()>
where
    R: io::Read + Send,
{
    let buffer = io::BufReader::new(read);

    buffer
        .for_byte_line(|line| {
            serialize_record(
                KindMarker::Data,
                RefContext::new(context, line).stream(&[
                    Item::Version(1),
                    Item::Tag(directive),
                    Item::Time(Utc::now().timestamp_nanos()),
                ]),
            )
            .and_then(|vec| tx_writer.send(Some(vec)).map_err(|e| e.into()))
            // Ugly workaround for closure's io::Error requirement,
            // Round trips from our local error into io::Error and back
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            .and(Ok(true))
        })
        .map_err(|e| e.into())
}

/// Helper function for serializing an iterator containing output data
fn serialize_record<'out, I, M>(r_kind: M, iter: I) -> Result<Vec<u8>>
where
    I: Iterator<Item = &'out Item<'out>>,
    M: Marker<Marker = KindMarker>,
{
    serde_cbor::to_vec(&RecordKind::new(
        r_kind.as_marker(),
        AsMapSerialize::new(iter),
    ))
    .map_err(|e| e.into())
}
