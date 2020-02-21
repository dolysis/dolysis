use {
    crate::spec::Record,
    bytes::{Bytes, BytesMut},
    futures::{
        channel::mpsc::{Receiver, Sender},
        prelude::*,
    },
    serde::{Deserialize, Serialize},
    std::{io, marker::PhantomData, pin::Pin},
    tokio::{io::AsyncWrite, stream},
    tokio_serde::{Deserializer, Framed, Serializer},
    tokio_util::codec,
};

pub struct RecordSink<S, T> {
    frame: Framed<S, Record, T, Cbor<Record, T>>,
}

impl<S, T> RecordSink<S, T>
where
    T: Serialize + std::marker::Unpin,
    S: Sink<Bytes> + std::marker::Unpin,
    S::Error: From<io::Error>,
{
    pub fn new(write: S) -> Self {
        Self {
            frame: Framed::new(write, Cbor::default()),
        }
    }

    pub fn block_on_send(&mut self, item: T) -> Result<(), S::Error> {
        futures::executor::block_on(self.frame.send(item))?;
        Ok(())
    }
}

pub fn cbor_sink<T>(tx_write: Sender<Bytes>, item: T) -> Result<(), Box<dyn std::error::Error>>
where
    T: Serialize + std::marker::Unpin,
{
    let mut sink: RecordSink<_, T> =
        RecordSink::new(tx_write.sink_map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
    sink.block_on_send(item)?;
    Ok(())
}

pub async fn cbor_write<W, I>(
    write: W,
    items: Receiver<Bytes>,
) -> Result<(), Box<dyn std::error::Error>>
where
    W: AsyncWrite,
{
    let sink = codec::FramedWrite::new(write, codec::LengthDelimitedCodec::new());
    items.map(|b| Ok(b)).forward(sink).await?;
    Ok(())
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
