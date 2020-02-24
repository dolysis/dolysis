use {
    crate::spec::Record,
    bytes::{Bytes, BytesMut},
    futures::prelude::*,
    serde::{Deserialize, Serialize},
    std::{
        io,
        marker::{PhantomData, Unpin},
        pin::Pin,
    },
    tokio::io::AsyncWrite,
    tokio_serde::{Deserializer, Serializer},
    tokio_util::codec,
};

// pub struct RecordInterface<KIND> {
//     kind: KIND,
// }

// impl<KIND> RecordInterface<KIND>
// where
//     KIND: Sink<Bytes>,
//     KIND::Error: From<io::Error>,
// {
//     pub fn new_sink(sink: KIND) -> Self {
//         Self { kind: sink }
//     }

//     pub fn as_sink<T>(&mut self) -> RecordSink2<KIND, T> {
//         RecordSink2 {
//             sink: &mut self.kind,
//             mkr: SymmetricalCbor::<T>::default(),
//         }
//     }
// }

// pub struct RecordSink2<'s, S, T>
// where
//     S: Sink<Bytes>,
//     S::Error: From<io::Error>,
// {
//     sink: &'s mut S,
//     mkr: SymmetricalCbor<T>,
// }

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
