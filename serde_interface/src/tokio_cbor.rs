use {
    crate::spec::Record,
    bytes::{Bytes, BytesMut},
    futures::{prelude::*, ready},
    pin_project::pin_project,
    serde::{Deserialize, Serialize},
    std::{
        io,
        marker::{PhantomData, Unpin},
        pin::Pin,
        task::{Context, Poll},
    },
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_serde::{Deserializer, Serializer},
    tokio_util::codec,
};

pub struct RecordInterface<KIND> {
    kind: KIND,
}

impl<KIND> RecordInterface<KIND>
where
    KIND: Sink<Bytes> + Unpin,
    KIND::Error: From<io::Error>,
{
    pub fn new_sink(sink: KIND) -> Self {
        Self { kind: sink }
    }

    pub fn as_sink<T>(&mut self) -> RecordSink2<KIND, T>
    where
        T: Serialize,
    {
        RecordSink2 {
            sink: Pin::new(&mut self.kind),
            mkr: SymmetricalCbor::<T>::default(),
        }
    }
}

impl<KIND> RecordInterface<KIND>
where
    KIND: TryStream<Ok = BytesMut> + Unpin,
    KIND::Error: From<io::Error>,
    BytesMut: From<KIND::Ok>,
{
    pub fn new_stream(stream: KIND) -> Self {
        Self { kind: stream }
    }

    pub fn as_stream(&mut self) -> RecordStream<KIND> {
        RecordStream {
            stream: Pin::new(&mut self.kind),
            mkr: SymmetricalCbor::<Record>::default(),
        }
    }
}

#[pin_project]
pub struct RecordStream<'s, St>
where
    St: TryStream<Ok = BytesMut>,
    St::Error: From<io::Error>,
    BytesMut: From<St::Ok>,
{
    #[pin]
    stream: Pin<&'s mut St>,
    #[pin]
    mkr: SymmetricalCbor<Record>,
}

impl<'s, St, E> Stream for RecordStream<'s, St>
where
    St: Stream<Item = Result<BytesMut, E>>,
    St: TryStream<Ok = BytesMut, Error = E>,
    E: From<io::Error>,
{
    type Item = Result<Record, St::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.as_mut().project().stream.poll_next(cx)) {
            Some(res) => match res {
                Ok(bytes) => {
                    Poll::Ready(Some(Ok(self.as_mut().project().mkr.deserialize(&bytes)?)))
                }
                Err(e) => Poll::Ready(Some(Err(e))),
            },
            None => Poll::Ready(None),
        }
    }
}

#[pin_project]
pub struct RecordSink2<'s, S, T>
where
    S: Sink<Bytes>,
    S::Error: From<io::Error>,
    T: Serialize,
{
    #[pin]
    sink: Pin<&'s mut S>,
    #[pin]
    mkr: SymmetricalCbor<T>,
}

impl<'s, S, T> Sink<T> for RecordSink2<'s, S, T>
where
    S: Sink<Bytes>,
    S::Error: From<io::Error>,
    T: Serialize,
{
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().sink.poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let res = self.as_mut().project().mkr.serialize(&item);
        let bytes = res.map_err(|e| e.into())?;

        self.as_mut().project().sink.start_send(bytes)?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().sink.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_flush(cx))?;
        self.project().sink.poll_close(cx)
    }
}

pub struct RecordSink<S>
where
    S: Sink<Bytes> + Unpin,
    S::Error: From<io::Error>,
{
    sink: S,
}

impl<S> RecordSink<S>
where
    S: Sink<Bytes> + Unpin,
    S::Error: From<io::Error>,
{
    pub fn new(sink: S) -> Self {
        Self { sink }
    }

    pub async fn sink_item<T>(&mut self, item: T) -> Result<(), S::Error>
    where
        T: Serialize + Unpin,
    {
        let datum = std::pin::Pin::new(&mut Cbor::<Record, _>::default()).serialize(&item)?;
        self.sink.send(datum).await
    }

    pub async fn sink_stream<T, St>(&mut self, stream: St) -> Result<(), S::Error>
    where
        T: Serialize + Unpin,
        St: Stream<Item = T>,
    {
        stream
            .map(|item| {
                std::pin::Pin::new(&mut Cbor::<Record, _>::default())
                    .serialize(&item)
                    .map_err(|e| e.into())
            })
            .forward(&mut self.sink)
            .await
    }
}

pub async fn cbor_write<W, S>(write: W, items: S) -> Result<(), io::Error>
where
    W: AsyncWrite,
    S: Stream<Item = Bytes>,
{
    let sink = codec::FramedWrite::new(write, codec::LengthDelimitedCodec::new());
    items.map(|b| Ok(b)).forward(sink).await?;
    Ok(())
}

// fn test<T>(io: T) -> Result<(), io::Error>
// where
//     T: AsyncRead + AsyncWrite,
// {
//     let framed = codec::Framed::new(io, codec::LengthDelimitedCodec::new());
//     let what = RecordInterface::new_sink(framed);

//     Ok(())
// }

/// Generic visitor that implements tokio-serde's De/Serialize traits
pub struct Cbor<Item, SinkItem> {
    _mkr: PhantomData<(Item, SinkItem)>,
}

impl<Item, SinkItem> Default for Cbor<Item, SinkItem> {
    fn default() -> Self {
        Self { _mkr: PhantomData }
    }
}

/// Note this is merely an alias for Cbor where the sink's
/// item is the same as the item that is serialized
pub type SymmetricalCbor<T> = Cbor<T, T>;

impl<Item, SinkItem> Deserializer<Item> for Cbor<Item, SinkItem>
where
    for<'a> Item: Deserialize<'a>,
{
    type Error = io::Error;

    fn deserialize(self: Pin<&mut Self>, src: &BytesMut) -> Result<Item, Self::Error> {
        serde_cbor::from_slice(src.as_ref()).map_err(|e| into_io_error(e))
    }
}

impl<Item, SinkItem> Serializer<SinkItem> for Cbor<Item, SinkItem>
where
    SinkItem: Serialize,
{
    type Error = io::Error;

    fn serialize(self: Pin<&mut Self>, item: &SinkItem) -> Result<Bytes, Self::Error> {
        serde_cbor::to_vec(item)
            .map_err(|e| into_io_error(e))
            .map(Into::into)
    }
}

fn into_io_error(cbor_err: serde_cbor::Error) -> io::Error {
    use {io::ErrorKind, serde_cbor::error::Category};

    match cbor_err.classify() {
        Category::Eof => io::Error::new(ErrorKind::UnexpectedEof, cbor_err),
        Category::Syntax => io::Error::new(ErrorKind::InvalidInput, cbor_err),
        Category::Data => io::Error::new(ErrorKind::InvalidData, cbor_err),
        Category::Io => io::Error::new(ErrorKind::Other, cbor_err),
    }
}
