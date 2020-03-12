//#![allow(dead_code)]

use {
    crate::{
        load::filter::{FilterSet, JoinSetHandle},
        models::{Data, DataContext, Header, HeaderContext, LocalRecord},
        prelude::{CrateResult as Result, *},
    },
    futures::{
        future::Either,
        prelude::*,
        ready,
        stream::{Peekable, Stream},
        task::{Context, Poll},
    },
    once_cell::sync::OnceCell,
    pin_project::pin_project,
    serde_interface::{Record, RecordInterface},
    std::collections::HashMap,
    std::{convert::TryFrom, pin::Pin, sync::Arc},
    tokio::{
        net::TcpListener,
        prelude::*,
        sync::{
            mpsc::{channel, Receiver, Sender},
            Barrier,
        },
        time::Duration,
    },
};

pub async fn listener(addr: &str) -> Result<()> {
    debug!("Listener is attempt to bind {}", addr);

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
                                .then(|stream| split_and_join(stream, tx_out));
                            let output = rx_out.for_each(|_item| future::ready(()));

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
    let unbound = RecordInterface::from_read(io::BufReader::new(socket));
    tokio::stream::StreamExt::timeout(unbound, Duration::from_secs(60))
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

type HandleMap = HashMap<String, (Sender<LocalRecord>, Sender<LocalRecord>, Arc<Barrier>)>;

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

async fn handle_header(header: Header, map: &mut HandleMap, mut output_tx: Sender<LocalRecord>) {
    match (header.cxt, map.contains_key(header.id.as_str())) {
        (HeaderContext::Start, false) => {
            let (out_tx, out_rx) = channel::<LocalRecord>(256);
            let (err_tx, err_rx) = channel::<LocalRecord>(256);
            let barrier = Arc::new(Barrier::new(3));

            map.insert(header.id.clone(), (out_tx, err_tx, barrier.clone()));

            // Send header to output
            output_tx
                .send(LocalRecord::Header(header))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;

            // Spawn join-er tasks
            tokio::spawn(handle_join(barrier.clone(), out_rx, output_tx.clone()));
            tokio::spawn(handle_join(barrier, err_rx, output_tx.clone()));
        }
        (HeaderContext::End, true) => {
            let (o, e, barrier) = map.remove(header.id.as_str()).unwrap();

            // Indicate to join-ers that input is finished
            drop((o, e));

            // Synchronize with join-ers
            barrier.wait().await;

            output_tx
                .send(LocalRecord::Header(header))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
        (HeaderContext::Start, true) => error!("Duplicate Header record (id: {})", &header.id),
        (HeaderContext::End, false) => error!(
            "Malformed stream, received Header end before start (id: {})",
            &header.id
        ),
    }
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

async fn handle_join(
    barrier: Arc<Barrier>,
    rx: Receiver<LocalRecord>,
    mut output_tx: Sender<LocalRecord>,
) {
    let handle = cli!().get_join().new_handle();
    let mut stream = rx.join_records(handle);

    while let Some(record) = stream.next().await {
        let _ = output_tx.send(record).await;
    }

    barrier.wait().await;
}

fn apply_recursive<'a, 'cli: 'a, St, I>(
    stream: St,
    mut ops: I,
) -> Box<dyn Stream<Item = St::Item> + 'a>
where
    St: Stream<Item = LocalRecord> + 'a,
    I: Iterator<Item = OpKind<'cli>>,
{
    match ops.next() {
        Some(OpKind::Filter(name)) => {
            let next = stream.filter_records(cli!().get_filter(), name);

            apply_recursive(next, ops)
        }
        None => Box::new(stream),
    }
}

enum OpKind<'cli> {
    Filter(&'cli str),
}

async fn filter(output_rx: Receiver<LocalRecord>) -> impl Stream<Item = LocalRecord> {
    output_rx.filter(|record| match record {
        LocalRecord::Header(_) => future::ready(true),
        LocalRecord::Data(ref data) => future::ready(cli!().get_filter().is_match_all(&data.data)),
    })
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
    first: OnceCell<()>,
    #[pin]
    inner: Peekable<St>,
}

impl<St> Stream for FirstLast<St>
where
    St: Stream,
{
    type Item = (bool, bool, St::Item);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let last = ready!(self.as_mut().project().inner.poll_peek(cx)).is_none();

        match ready!(self.as_mut().project().inner.poll_next(cx)) {
            Some(item) => {
                let first = self.first.set(()).is_ok();
                Poll::Ready(Some((first, last, item)))
            }
            None => Poll::Ready(None),
        }
    }
}

trait JoinRecords: Stream + Sized {
    fn join_records<'cli>(self, handle: JoinSetHandle<'cli>) -> Join<Self>;
}

impl<St> JoinRecords for St
where
    St: Stream<Item = LocalRecord>,
{
    fn join_records<'cli>(self, handle: JoinSetHandle<'cli>) -> Join<Self> {
        Join {
            inner: self,
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

        match ready!(this.as_mut().project().inner.poll_next(cx)) {
            None => Poll::Ready(None),
            Some(record) => match record {
                header @ LocalRecord::Header(_) => Poll::Ready(Some(header)),
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
                        (false, false) => Poll::Ready(Some(LocalRecord::Data(data))),
                        // No ongoing join, but the current record IS a join... set it as the ongoing join
                        (false, true) => {
                            this.as_mut().project().ongoing.replace(data);
                            Poll::Pending
                        }
                        // Ongoing join, which has now finished because the current record IS NOT a join
                        (true, false) => {
                            let data = this
                                .project()
                                .ongoing
                                .take()
                                .map(|data| LocalRecord::Data(data));
                            Poll::Ready(data)
                        }
                        // Ongoing join, which will continue as the current record is a join
                        (true, true) => {
                            this.project()
                                .ongoing
                                .as_mut()
                                // Append a newline and extend the base data with the current data
                                // Note that copied() here does not copy data only a reference
                                .map(|on| {
                                    on.data.extend(["\n", data.data.as_str()].iter().copied())
                                });
                            Poll::Pending
                        }
                    }
                }
            },
        }
    }
}

trait FilterRecords: Stream + Sized {
    fn filter_records<'cli>(self, set: &'cli FilterSet, key: &'cli str)
        -> RecordFilter<'cli, Self>;
}

impl<St> FilterRecords for St
where
    St: Stream<Item = LocalRecord>,
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

        match ready!(this.as_mut().project().inner.poll_next(cx)) {
            Some(record) => match record {
                header @ LocalRecord::Header(_) => Poll::Ready(Some(header)),
                LocalRecord::Data(data) if this.set.is_match_with(this.filter_name, &data.data) => {
                    Poll::Ready(Some(LocalRecord::Data(data)))
                }
                LocalRecord::Data(_) => Poll::Pending,
            },
            None => Poll::Ready(None),
        }
    }
}
