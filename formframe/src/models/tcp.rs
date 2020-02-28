use {
    crate::prelude::{CrateResult as Result, *},
    futures::{
        pin_mut,
        prelude::*,
        ready,
        stream::{Peekable, Stream},
        task::{Context, Poll},
    },
    once_cell::sync::OnceCell,
    pin_project::pin_project,
    serde_interface::{Record, RecordInterface},
    std::collections::HashMap,
    std::pin::Pin,
    tokio::{
        net::TcpListener,
        prelude::*,
        sync::mpsc::{channel, Receiver, Sender},
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
            .inspect_ok(|(_, client)| debug!("Accepted connection from: {}", client))
            .inspect_err(|e| error!("Failed to accept connection: {}", e))
            .map_ok_or_else(
                |_| (),
                |(socket, client)| {
                    tokio::spawn(
                        async move {
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
                                    .map(|(_, _, record)| record)
                                    .scan(HashMap::<String, Sender<Record>>::new(), |map, record| {
                                        let out = future::ready(Some(()));
                                        match record {
                                            Record::Header(_) => unimplemented!(), // spawn task and pass it a channel rx while storing the tx in the hashmap
                                            Record::Data(_) => unimplemented!(), // 
                                            _ => out
                                        }
                                    })
                                    .collect::<()>()
                                    .await;
                        }
                        .instrument(always_span!("tcp.client", client = %client)),
                    );
                },
            )
            .await
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
