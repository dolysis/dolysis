//#![allow(dead_code)]

use {
    crate::{
        cli::OpKind,
        load::filters::{FilterSet, JoinSetHandle},
        models::{Data, DataContext, Header, HeaderContext, LocalRecord},
        prelude::{CrateResult as Result, *},
    },
    futures::{
        pin_mut,
        prelude::*,
        ready,
        stream::{Peekable, Stream},
        task::{Context, Poll},
    },
    lib_transport::{Record, RecordFrame, RecordInterface, SymmetricalCbor},
    once_cell::sync::OnceCell,
    pin_project::pin_project,
    std::{collections::HashMap, iter::FromIterator},
    std::{convert::TryFrom, pin::Pin},
    tokio::{
        net::{TcpListener, TcpStream, ToSocketAddrs},
        sync::{
            broadcast,
            mpsc::{channel, Receiver, Sender},
        },
        task::JoinHandle,
        time::Duration,
    },
    tokio_serde::Serializer,
};

pub async fn listener(addr: impl ToSocketAddrs) -> Result<()> {
    let mut listener = TcpListener::bind(addr)
        .inspect_ok(|tcp| {
            tcp.local_addr()
                .map(|fixed| info!("Success, listening at: {}", fixed))
                .unwrap_or_else(|e| {
                    warn!("Success, however failed to resolve local address: {}", e)
                })
        })
        .await
        .map_err(|e| e.into())
        .log(Level::ERROR)?;

    loop {
        listener
            .accept()
            .map_ok_or_else(
                |e| warn!("Failed to accept connection: {}", e),
                |(socket, client)| {
                    debug!("Accepted connection from: {}", client);

                    tokio::spawn(
                        async move {
                            let (tx_out, rx_out) = channel::<LocalRecord>(256);
                            let input = handle_connection(socket)
                                .then(|stream| split_and_join(stream, tx_out))
                                .instrument(always_span!("con.input"))
                                .map(|_| ());
                            let output =
                                handle_output(rx_out).instrument(always_span!("con.output"));

                            // Await both the joined records and the final output
                            tokio::join!(tokio::spawn(input), tokio::spawn(output))
                        }
                        .instrument(always_span!("tcp.handler", client = %client)),
                    );
                },
            )
            .await
    }
}

async fn handle_connection<T>(socket: T) -> impl Stream<Item = LocalRecord>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite,
{
    let unbound = RecordInterface::from_read(socket);
    tokio::stream::StreamExt::timeout(unbound, Duration::from_secs(3))
        .inspect(|record| debug!("=> {:?}", record))
        .take_while(|timer| future::ready(timer.is_ok()))
        .filter_map(|res| match res.unwrap() {
            Ok(record) => future::ready(Some(record)),
            Err(e) => future::ready({
                warn!(
                    "Invalid record detected in stream: {}... ignoring",
                    e
                );
                None
            }),
        })
        .first_last()
        .inspect(|(first, last, _)| debug!(first, last))
        .take_while(|(first, last, record)| future::ready(match record {
            Record::StreamStart if !first => {
                error!("Malformed stream, client sent: 'Stream Start' out of sequence... terminating connection");
                false
            }
            Record::StreamEnd if !last => {
                error!("Malformed stream, client sent: 'Stream End' out of sequence... terminating connection");
                false
            }
            _ => true
        }))
        .filter_map(|(_, _, record)| future::ready(match record {
            Record::Header(rcd) => LocalRecord::try_from(rcd).inspect(|res| if let Err(e) = res {
                warn!("{}... discarding record", e)
            }).ok(),
            Record::Data(rcd) => LocalRecord::try_from(rcd).inspect(|res| if let Err(e) = res {
                warn!("{}... discarding record", e)
            }).ok(),
            other => {info!(kind = %other.span_display(), "Discarding record"); None}
        }))
}

type HandleMap = HashMap<
    String,
    (
        Sender<LocalRecord>,
        Sender<LocalRecord>,
        (JoinHandle<()>, JoinHandle<()>),
    ),
>;

async fn split_and_join<St>(stream: St, output_tx: Sender<LocalRecord>)
where
    St: Stream<Item = LocalRecord>,
{
    let mut map = HandleMap::new();
    futures::pin_mut!(stream);

    while let Some(record) = stream.next().await {
        match record {
            LocalRecord::Header(header) => handle_header(header, &mut map, output_tx.clone()).await,
            LocalRecord::Data(data) => handle_data(data, &mut map).await,
        }
    }
}

async fn handle_header(header: Header, map: &mut HandleMap, output_tx: Sender<LocalRecord>) {
    match (header.cxt, map.contains_key(header.id.as_str())) {
        (HeaderContext::Start, false) => header_start(header, map, output_tx).await,
        (HeaderContext::End, true) => header_end(header, map, output_tx).await,
        (HeaderContext::Start, true) => error!("Duplicate Header record (id: {})", &header.id),
        (HeaderContext::End, false) => error!(
            "Malformed stream, received Header end before start (id: {})",
            &header.id
        ),
    }
}

async fn header_start(header: Header, map: &mut HandleMap, mut output_tx: Sender<LocalRecord>) {
    let (out_tx, out_rx) = channel::<LocalRecord>(256);
    let (err_tx, err_rx) = channel::<LocalRecord>(256);

    // Spawn join-er tasks
    let stdout =
        tokio::spawn(handle_stream(out_rx, output_tx.clone()).instrument(always_span!("stdout")));
    let stderr =
        tokio::spawn(handle_stream(err_rx, output_tx.clone()).instrument(always_span!("stderr")));

    map.insert(header.id.clone(), (out_tx, err_tx, (stdout, stderr)));

    trace!(id = header.id.as_str(), "Added stream to map");

    // Send header to output
    output_tx
        .send(LocalRecord::Header(header))
        .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
        .await;
}

async fn header_end(header: Header, map: &mut HandleMap, mut output_tx: Sender<LocalRecord>) {
    let (o, e, barrier) = map.remove(header.id.as_str()).unwrap();
    let id = header.id.as_str();
    // Indicate to join-ers that input is finished
    drop((o, e));

    // Synchronize with join-ers
    trace!(id, "Just before waiting on stdout/err streams");
    let (_, _) = tokio::join!(barrier.0, barrier.1);

    trace!(id, "Removed stream from map");

    output_tx
        .send(LocalRecord::Header(header))
        .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
        .await;
}

async fn handle_data(data: Data, map: &mut HandleMap) {
    match (data.cxt, map.contains_key(data.id.as_str())) {
        (DataContext::Stdout, true) => {
            map.get_mut(data.id.as_str())
                .unwrap()
                .0
                .send(LocalRecord::Data(data))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
        (DataContext::Stderr, true) => {
            map.get_mut(data.id.as_str())
                .unwrap()
                .1
                .send(LocalRecord::Data(data))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
        _ => warn!(
            "Data record (id: {}) sent out of sequence... discarding",
            &data.id
        ),
    }
}

async fn handle_stream(rx: Receiver<LocalRecord>, mut output_tx: Sender<LocalRecord>) {
    let stream = rx.inspect(|record| trace!("pre-ops: {:?}", &record));
    let mut stream = apply_ops(stream, cli!().get_exec_list().get_ops());

    while let Some(record) = stream.next().await {
        trace!("post-ops: {:?}", &record);
        let _ = output_tx.send(record).await;
    }
}

fn apply_ops<'a, 'cli: 'a, St: 'a, I>(
    stream: St,
    ops: Option<I>,
) -> Box<dyn Stream<Item = LocalRecord> + Unpin + Send + 'a>
where
    St: Stream<Item = LocalRecord> + Unpin + Send,
    I: Iterator<Item = OpKind<'cli>>,
{
    match ops {
        Some(ops) => ops.fold(Box::new(stream), |state, op| match op {
            OpKind::Join => Box::new(state.join_records(cli!().get_join().new_handle())),
            OpKind::Filter(name) => Box::new(state.filter_records(cli!().get_filter(), name)),
        }),
        None => Box::new(stream),
    }
}

async fn handle_output(output_rx: Receiver<LocalRecord>) -> Result<()> {
    let loaders = cli!()
        .get_exec_list()
        .get_loaders()
        .map(|iter| {
            iter.fold(broadcast::channel(256), |(tx, rx), load| {
                tokio::spawn(
                    spawn_loader(load.0, rx).instrument(always_span!("loader", addr = load.0)),
                );

                let new_rx = tx.subscribe();
                (tx, new_rx)
            })
        })
        .map(|(tx, _)| tx);

    match loaders {
        Some(tx) => {
            pin_mut!(tx);
            stream::once(future::ready(Record::StreamStart))
                .chain(output_rx.map(|local| local.into()))
                .chain(stream::once(future::ready(Record::StreamEnd)))
                .map(|record| {
                    let mkr = SymmetricalCbor::<Record>::default();
                    pin_mut!(mkr);
                    Serializer::serialize(mkr, &record).map_err(CrateError::from)
                })
                // Due to a [compiler bug](https://github.com/rust-lang/rust/issues/64552) as of 2020/03/23 we must box this stream.
                // The bug occurs due to the compiler erasing certain lifetime bounds in a generator (namely 'static ones) leading to the false
                // assumption that lifetime 'a: 'static and 'b: 'static do not live as long as each other. This leads to inscrutable error messages.
                // TODO: Once said issue is resolved remove this allocation.
                .boxed()
                .try_for_each(|serialized_record| {
                    future::ready(tx.send(serialized_record)).map(|_| Ok(()))
                })
                .await
        }
        None => {
            output_rx
                .map(|record| -> Record { record.into() })
                .map(Ok)
                // See the Some() branch's comment for an explanation
                .boxed()
                .forward(RecordInterface::from_write(tokio::io::sink()))
                .await?;

            Ok(())
        }
    }
}

async fn spawn_loader<T>(addr: &'static str, output_rx: broadcast::Receiver<T>) -> Result<()>
where
    T: Clone + IntoIterator<Item = u8>,
{
    let socket = TcpStream::connect(addr).await?;
    let sink = RecordFrame::write(socket);
    output_rx
        .take_while(|res| match res {
            Err(e) if *e == broadcast::RecvError::Closed => future::ready(false),
            _ => future::ready(true),
        })
        .filter_map(|res| async {
            match res {
                Ok(item) => Some(item),
                Err(broadcast::RecvError::Lagged(missed)) => {
                    warn!("Loader is slow, {} records skipped...", missed);
                    None
                }
                _ => None,
            }
        })
        // Note this into_iter / from_iter BS works around dependencies (tokio_serde + tokio_util) not reexporting the version of
        // [bytes](https://docs.rs/bytes/) they use, leading to version mismatch errors on dependency updates. This "fix" likely has a runtime cost,
        // but its advantage is that dep updates don't randomly break code.
        // TODO: raise issues on the deps to properly reexport their public types
        .map(|item| FromIterator::from_iter(item.into_iter()))
        .map(Ok)
        .forward(sink)
        .await?;

    Ok(())
}

pub trait FindFirstLast: Stream + Sized {
    fn first_last(self) -> FirstLast<Self>;
}

impl<St> FindFirstLast for St
where
    St: Stream,
{
    fn first_last(self) -> FirstLast<Self> {
        FirstLast {
            first: OnceCell::new(),
            inner: self.peekable(),
            item: None,
        }
    }
}

/// A stream that checks whether the current item is the first or last item in the stream.
/// Be aware that this type makes use of a peekable stream and as such every item in the stream
/// awaits the item after it, not itself, i.e: given a stream `[1 ,2 ,3 ,4]`, `1` will only be returned
/// once `2` as been successfully polled
#[pin_project]
pub struct FirstLast<St>
where
    St: Stream,
{
    #[pin]
    inner: Peekable<St>,
    first: OnceCell<()>,
    item: Option<St::Item>,
}

impl<St> Stream for FirstLast<St>
where
    St: Stream,
{
    type Item = (bool, bool, St::Item);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self;

        // If we have an item already, don't poll_next, just see if poll_peek is ready
        if this.item.is_some() {
            let last = ready!(this.as_mut().project().inner.poll_peek(cx)).is_none();
            let item = this.as_mut().project().item.take().unwrap();
            let first = this.first.set(()).is_ok();
            return Poll::Ready(Some((first, last, item)));
        }

        // Else get the next item in the stream
        match ready!(this.as_mut().project().inner.poll_next(cx)) {
            Some(item) => match this.as_mut().project().inner.poll_peek(cx) {
                Poll::Pending => {
                    // If the peek isn't ready, place the current item into local storage
                    *this.as_mut().project().item = Some(item);
                    Poll::Pending
                }
                Poll::Ready(peek) => {
                    let last = peek.is_none();
                    let first = this.first.set(()).is_ok();
                    Poll::Ready(Some((first, last, item)))
                }
            },
            None => Poll::Ready(None),
        }
    }
}

trait JoinRecords: Stream + Sized {
    fn join_records(self, handle: JoinSetHandle<'_>) -> Join<Self>;
}

impl<St> JoinRecords for St
where
    St: Stream,
{
    fn join_records(self, handle: JoinSetHandle<'_>) -> Join<Self> {
        Join {
            inner: self,
            overflow: None,
            ongoing: None,
            handle,
        }
    }
}

#[pin_project]
struct Join<'j, St>
where
    St: Stream,
{
    #[pin]
    inner: St,
    overflow: Option<Data>,
    ongoing: Option<Data>,
    handle: JoinSetHandle<'j>,
}

impl<St> Stream for Join<'_, St>
where
    St: Stream<Item = LocalRecord>,
{
    type Item = LocalRecord;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self;

        // If the last call had overflow data, return it before polling for the next item
        if this.overflow.is_some() {
            return Poll::Ready(
                this.as_mut()
                    .project()
                    .overflow
                    .take()
                    .map(LocalRecord::Data),
            );
        }

        loop {
            match ready!(this.as_mut().project().inner.poll_next(cx)) {
                None => return Poll::Ready(None),
                Some(record) => match record {
                    header @ LocalRecord::Header(_) => return Poll::Ready(Some(header)),
                    LocalRecord::Data(data) => {
                        // There are 4 possible outcomes for a Data record depending of the state of
                        // (A, B) where A and B are bools and represent:
                        // A: Whether we currently have an ongoing join
                        // B: Whether the current record should be joined
                        match (
                            this.ongoing.is_some(),
                            this.as_mut()
                                .project()
                                .handle
                                .should_join(data.data.as_str()),
                        ) {
                            // No ongoing join & current record is not a join
                            (false, false) => return Poll::Ready(Some(LocalRecord::Data(data))),
                            // No ongoing join, but the current record IS a join... set it as the ongoing join
                            (false, true) => *this.as_mut().project().ongoing = Some(data),
                            // Ongoing join, which has now finished because the current record IS NOT a join
                            (true, false) => {
                                // Put the overflow item in local storage
                                *this.as_mut().project().overflow = Some(data);
                                let join = this.project().ongoing.take().map(LocalRecord::Data);
                                return Poll::Ready(join);
                            }
                            // Ongoing join, which will continue as the current record is a join
                            (true, true) => {
                                // Append a newline and extend the base data with the current data
                                // Note that copied() here does not copy data only a reference
                                if let Some(ongoing) = this.as_mut().project().ongoing.as_mut() {
                                    ongoing
                                        .data
                                        .extend(["\n", data.data.as_str()].iter().copied())
                                };
                            }
                        }
                    }
                },
            }
        }
    }
}

trait FilterRecords: Stream + Sized {
    fn filter_records<'cli>(self, set: &'cli FilterSet, key: &'cli str)
        -> RecordFilter<'cli, Self>;
}

impl<St> FilterRecords for St
where
    St: Stream,
{
    fn filter_records<'cli>(
        self,
        set: &'cli FilterSet,
        key: &'cli str,
    ) -> RecordFilter<'cli, Self> {
        RecordFilter {
            inner: self,
            filter_name: key,
            set,
        }
    }
}

#[pin_project]
struct RecordFilter<'f, St>
where
    St: Stream,
{
    #[pin]
    inner: St,
    filter_name: &'f str,
    set: &'f FilterSet,
}

impl<St> Stream for RecordFilter<'_, St>
where
    St: Stream<Item = LocalRecord>,
{
    type Item = St::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self;

        loop {
            match ready!(this.as_mut().project().inner.poll_next(cx)) {
                Some(record) => match record {
                    header @ LocalRecord::Header(_) => return Poll::Ready(Some(header)),
                    LocalRecord::Data(record) => {
                        if this.set.is_match_with(this.filter_name, &record.data) {
                            trace!(data = %record.data, "MATCH");
                            return Poll::Ready(Some(LocalRecord::Data(record)));
                        } else {
                            trace!(data = %record.data, "NO MATCH");
                        }
                    }
                },
                None => return Poll::Ready(None),
            }
        }
    }
}
