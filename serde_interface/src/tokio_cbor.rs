use {
    crate::record::Record,
    futures::{pin_mut, prelude::*, ready},
    pin_project::pin_project,
    serde::Serialize,
    std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    },
    tokio::io::{AsyncRead, AsyncWrite},
    tokio_serde::{Deserializer, Serializer},
    tokio_util::codec::{Framed, FramedRead, FramedWrite, LengthDelimitedCodec},
};

pub use {
    bytes::{Bytes, BytesMut},
    tokio_serde::formats::{Cbor, SymmetricalCbor},
};

/// Contains convenience methods for generating framed readers/writers
pub struct RecordFrame;

impl RecordFrame {
    /// Framed variant that is read and write
    pub fn read_write<T>(io: T) -> Framed<T, LengthDelimitedCodec>
    where
        T: AsyncRead + AsyncWrite,
    {
        Framed::new(io, LengthDelimitedCodec::default())
    }

    /// Read only variant
    pub fn read<T>(io: T) -> FramedRead<T, LengthDelimitedCodec>
    where
        T: AsyncRead,
    {
        FramedRead::new(io, LengthDelimitedCodec::default())
    }

    /// Write only variant
    pub fn write<T>(io: T) -> FramedWrite<T, LengthDelimitedCodec>
    where
        T: AsyncWrite,
    {
        FramedWrite::new(io, LengthDelimitedCodec::default())
    }
}

/// Provides an interface for moving from deserialized Records to serialized
/// byte buffers and vice versa.
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
        let bytes = mkr.serialize(&item)?;

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
