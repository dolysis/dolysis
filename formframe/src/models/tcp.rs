//#![allow(dead_code)]

use {
    crate::{
        load::filter::JoinSetHandle,
        models::{Data, Header, HeaderContext, LocalRecord},
        prelude::{CrateResult as Result, *},
    },
    futures::{
        prelude::*,
        ready,
        stream::{Peekable, Stream},
        task::{Context, Poll},
    },
    once_cell::sync::OnceCell,
    pin_project::pin_project,
    serde_interface::{Record, RecordInterface},
    std::collections::HashMap,
    std::{convert::TryFrom, pin::Pin},
    tokio::{
        net::TcpListener,
        prelude::*,
        sync::{
            mpsc::{channel, Receiver, Sender},
            oneshot,
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
                |e| error!("Failed to accept connection: {}", e),
                |(socket, client)| {
                    debug!("Accepted connection from: {}", client);

                    tokio::spawn(
                        async move {
                            let (tx_out, rx_out) = channel::<LocalRecord>(256);
                            let input =
                                handle_connection(socket).then(|stream| join_with(stream, tx_out));
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

async fn join_with<St>(stream: St, joined_tx: Sender<LocalRecord>)
where
    St: Stream<Item = LocalRecord>,
{
    let mut map = HashMap::<String, Sender<LocalRecord>>::new();
    futures::pin_mut!(stream);

    while let Some(record) = stream.next().await {
        match record {
            LocalRecord::Header(header) => {
                if map.contains_key(header.id.as_str()) {
                    if header.cxt == HeaderContext::Start {
                        warn!("Detected duplicate header ID...");
                    }
                } else {
                    let (mut tx, rx) = channel::<LocalRecord>(256);
                    let id = header.id.clone();
                    tx.send(LocalRecord::Header(header))
                        .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                        .await;
                    map.insert(id, tx);
                    tokio::spawn(handle_join(rx, joined_tx.clone()));
                }
            }
            LocalRecord::Data(data) => {
                let id = data.id.as_str();
                if map.contains_key(id) {
                    map.get_mut(id)
                        .unwrap()
                        .send(LocalRecord::Data(data))
                        .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                        .await;
                } else {
                    warn!("Record stream out of sequence, data record received before header, discarding")
                }
            }
        }
    }
}

async fn handle_join(rx: Receiver<LocalRecord>, mut output_tx: Sender<LocalRecord>) {
    let mut fused = rx.fuse();
    let mut handle = cli!().get_join().new_handle();
    let mut ongoing = None;

    while let Some(record) = fused.next().await {
        match record {
            LocalRecord::Header(header) => join_header(&mut output_tx, &mut ongoing, header).await,
            LocalRecord::Data(data) => {
                join_data(&mut output_tx, &mut handle, &mut ongoing, data).await
            }
        }
    }
}

async fn join_header(tx: &mut Sender<LocalRecord>, ongoing: &mut Option<Data>, header: Header) {
    match header.cxt {
        HeaderContext::Start => {
            tx.send(LocalRecord::Header(header))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
        HeaderContext::End => {
            // Flush any remaining data
            if ongoing.is_some() {
                tx.send(LocalRecord::Data(ongoing.take().unwrap()))
                    .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                    .await;
            }

            tx.send(LocalRecord::Header(header))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
    }
}

async fn join_data(
    tx: &mut Sender<LocalRecord>,
    handle: &mut JoinSetHandle<'_>,
    ongoing: &mut Option<Data>,
    data: Data,
) {
    // There are 4 possible outcomes for a Data record depending of the state of
    // (A, B) where A and B are bools and represent:
    // A: Whether we are currently have an ongoing join
    // B: Whether the current record should be joined
    match (ongoing.is_some(), handle.should_join(&*data.data.as_str())) {
        // No ongoing join & current record is not a join
        (false, false) => {
            tx.send(LocalRecord::Data(data))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
        // No ongoing join, but the current record IS a join... set it as the ongoing join
        (false, true) => {
            ongoing.replace(data);
        }
        // Ongoing join, which has now finished because the current record IS NOT a join
        (true, false) => {
            let joined = ongoing.take().unwrap();

            tx.send(LocalRecord::Data(joined))
                .unwrap_or_else(|e| error!("join TX closed unexpectedly: {}", e))
                .await;
        }
        // Ongoing join, which will continue as the current record is a join
        (true, true) => {
            ongoing
                .as_mut()
                // Append a newline and extend the base data with the current data
                // Note that copied() here does not copy data only a reference
                .map(|on| on.data.extend(["\n", data.data.as_str()].iter().copied()));
        }
    }
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
