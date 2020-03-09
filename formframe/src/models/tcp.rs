//#![allow(dead_code)]

use {
    crate::{
        models::{Data, LocalRecord},
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
                            let (tx_out, rx_out) = channel::<usize>(256);
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

async fn join_with<St>(stream: St, joined_tx: Sender<usize>)
where
    St: Stream<Item = LocalRecord>,
{
    let mut map = HashMap::<String, Sender<LocalRecord>>::new();
    futures::pin_mut!(stream);

    while let Some(record) = stream.next().await {
        match record {
            LocalRecord::Header(header) => {
                if map.contains_key(header.id.as_str()) {
                    warn!("Detected duplicate header ID...");
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

async fn handle_join(_rx: Receiver<LocalRecord>, joined_tx: Sender<usize>) {
    unimplemented!()
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
