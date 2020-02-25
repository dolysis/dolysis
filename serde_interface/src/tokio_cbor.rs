use {
    crate::spec::Record,
    bytes::{Bytes, BytesMut},
    futures::{pin_mut, prelude::*, ready},
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
    tokio_util::codec::{Framed, FramedRead, FramedWrite, LengthDelimitedCodec},
};

pub struct RecordFrame;

impl RecordFrame {
    pub fn read_write<T>(io: T) -> RecordFrameBoth<T>
    where
        T: AsyncRead + AsyncWrite,
    {
        RecordFrameBoth::new(io, LengthDelimitedCodec::default())
    }

    pub fn read<T>(io: T) -> RecordFrameRead<T>
    where
        T: AsyncRead,
    {
        RecordFrameRead::new(io, LengthDelimitedCodec::default())
    }

    pub fn write<T>(io: T) -> RecordFrameWrite<T>
    where
        T: AsyncWrite,
    {
        RecordFrameWrite::new(io, LengthDelimitedCodec::default())
    }
}

pub type RecordFrameBoth<T> = Framed<T, LengthDelimitedCodec>;
pub type RecordFrameRead<T> = FramedRead<T, LengthDelimitedCodec>;
pub type RecordFrameWrite<T> = FramedWrite<T, LengthDelimitedCodec>;

#[pin_project]
pub struct RecordInterface<IF> {
    #[pin]
    inner: IF,
}

impl<IF> RecordInterface<IF>
where
    IF: TryStream<Ok = BytesMut>,
    IF: Sink<Bytes>,
    <IF as TryStream>::Error: From<io::Error>,
    <IF as Sink<Bytes>>::Error: From<io::Error>,
{
    /// Generates an Interface that implements both `Sink<T: Serialize>` and `TryStream<Ok = Record>`
    /// from an underlying object. Most commonly, a
    /// `RecordFrame` instance.
    /// It is useful for situations when you need to both deserialize and serialize `Record`s.
    /// If you only have the async IO stream (i.e a type that is `AsyncRead + AsyncWrite`)
    /// prefer using `RecordInterface::from_both`
    pub fn new_both(inner: IF) -> Self {
        Self { inner }
    }
}

impl<IF> RecordInterface<IF>
where
    IF: TryStream<Ok = BytesMut>,
    IF::Error: From<io::Error>,
{
    /// Generates an Interface that implements `TryStream<Ok = Record>`
    /// This function is useful when the IO stream is being handled further
    /// up the data stream, for example, if your data stream looks like this: TCP Socket -> `RecordFrameRead` -> `map()` -> `channel` -> ... -> `RecordInterface`
    ///
    /// If you only have the async IO stream (i.e a type that is at least `AsyncRead`)
    /// prefer using `RecordInterface::from_write`
    pub fn new_stream(inner: IF) -> Self {
        Self { inner }
    }
}

impl<IF> RecordInterface<IF>
where
    IF: Sink<Bytes>,
    IF::Error: From<io::Error>,
{
    /// Generates an Interface that implements `Sink<T: Serialize>` from an underlying
    /// sink, most commonly a `RecordFrameWrite`
    /// This function is useful when the IO stream is further down the data stream, i.e `RecordInterface` -> `channel` -> `inspect()` -> TCP Socket.
    ///
    /// If you only have the async IO stream (i.e a type that is at least `AsyncWrite`)
    /// prefer using `RecordInterface::from_read`
    pub fn new_sink(inner: IF) -> Self {
        Self { inner }
    }
}

impl<T> RecordInterface<Framed<T, LengthDelimitedCodec>>
where
    T: AsyncRead + AsyncWrite,
{
    /// Generates an Interface that implements both `Sink<T: Serialize>` and `TryStream<Ok = Record>`
    /// this function requires that the underlying io type is `AsyncRead + AsyncWrite`
    pub fn from_both(io: T) -> Self {
        Framed::new(io, LengthDelimitedCodec::new()).into()
    }
}

impl<T> RecordInterface<FramedWrite<T, LengthDelimitedCodec>>
where
    T: AsyncWrite,
{
    /// Generates a write only Interface that implements `Sink<T: Serialize>`
    /// this function only requires that the underlying io type is `AsyncWrite`
    pub fn from_write(io: T) -> Self {
        FramedWrite::new(io, LengthDelimitedCodec::new()).into()
    }
}

impl<T> RecordInterface<FramedRead<T, LengthDelimitedCodec>>
where
    T: AsyncRead,
{
    /// Generates a read only Interface that implements `TryStream<Ok = Record>`
    /// this function only requires that the underlying io type is `AsyncRead`
    pub fn from_read(io: T) -> Self {
        FramedRead::new(io, LengthDelimitedCodec::new()).into()
    }
}

impl<T> From<Framed<T, LengthDelimitedCodec>> for RecordInterface<Framed<T, LengthDelimitedCodec>>
where
    T: AsyncRead + AsyncWrite,
{
    fn from(framed_io: Framed<T, LengthDelimitedCodec>) -> Self {
        RecordInterface::new_both(framed_io)
    }
}

impl<T> From<FramedRead<T, LengthDelimitedCodec>>
    for RecordInterface<FramedRead<T, LengthDelimitedCodec>>
where
    T: AsyncRead,
{
    fn from(framed_io: FramedRead<T, LengthDelimitedCodec>) -> Self {
        RecordInterface::new_stream(framed_io)
    }
}

impl<T> From<FramedWrite<T, LengthDelimitedCodec>>
    for RecordInterface<FramedWrite<T, LengthDelimitedCodec>>
where
    T: AsyncWrite,
{
    fn from(framed_io: FramedWrite<T, LengthDelimitedCodec>) -> Self {
        RecordInterface::new_sink(framed_io)
    }
}

impl<IF, E> Stream for RecordInterface<IF>
where
    IF: Stream<Item = Result<BytesMut, E>>,
    IF: TryStream<Ok = BytesMut, Error = E>,
    E: From<io::Error>,
{
    type Item = Result<Record, IF::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(self.as_mut().project().inner.poll_next(cx)) {
            Some(res) => match res {
                Ok(bytes) => {
                    let mkr = SymmetricalCbor::<Record>::default();
                    pin_mut!(mkr);
                    Poll::Ready(Some(Ok(mkr.deserialize(&bytes)?)))
                }
                Err(e) => Poll::Ready(Some(Err(e))),
            },
            None => Poll::Ready(None),
        }
    }
}

impl<IF, T> Sink<T> for RecordInterface<IF>
where
    IF: Sink<Bytes>,
    IF::Error: From<io::Error>,
    T: Serialize,
{
    type Error = IF::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let mkr = SymmetricalCbor::<T>::default();
        pin_mut!(mkr);
        let res = mkr.serialize(&item);
        let bytes = res.map_err(|e| e.into())?;

        self.as_mut().project().inner.start_send(bytes)?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().project().inner.poll_flush(cx))?;
        self.project().inner.poll_close(cx)
    }
}

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
