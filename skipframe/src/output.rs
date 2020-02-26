use {
    crate::prelude::*,
    serde::ser::{SerializeMap, Serializer},
    serde::Serialize,
    serde_interface::{DataContext, Marker, TagMarker},
    std::{fmt, sync::Arc},
};

/// Contextual information for interpreting associated data
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "DataContext")]
pub enum Directive {
    Begin,
    Stdout,
    Stderr,
    End,
}

impl Into<DataContext> for Directive {
    fn into(self) -> DataContext {
        match self {
            Self::Begin => DataContext::Start,
            Self::Stdout => DataContext::Stdout,
            Self::Stderr => DataContext::Stderr,
            Self::End => DataContext::End,
        }
    }
}

impl std::fmt::Display for Directive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let x = match self {
            Self::Begin => "BEGIN",
            Self::Stdout => "STDOUT",
            Self::Stderr => "STDERR",
            Self::End => "END",
        };

        write!(f, "{}", x)
    }
}

impl SpanDisplay for Directive {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let x = match self {
            Self::Begin => "Begin",
            Self::Stdout => "Stdout",
            Self::Stderr => "Stderr",
            Self::End => "End",
        };

        write!(f, "{}", x)
    }
}

/// Container for various relevant data that should be passed to the parser
pub struct OutputContext {
    // Note the 'static prevents adding non-owned variants
    // TODO: Replace Vec with ArrayVec (https://docs.rs/arrayvec/0.5.1/arrayvec/index.html)
    inner: Vec<Item<'static>>,
}

impl OutputContext {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn insert_id(&mut self, id: &str) {
        self.inner.push(Item::Id(Arc::from(id)))
    }

    pub fn insert_pid(&mut self, pid: u32) {
        self.inner.push(Item::Pid(pid))
    }

    pub fn insert_version(&mut self, version: u32) {
        self.inner.push(Item::Version(version))
    }

    // pub fn as_ref(&self) -> &[Item] {
    //     &self.inner
    // }

    pub fn stream<'a, 'b: 'a>(
        &'b self,
        header: &'a [Item<'b>],
    ) -> impl Iterator<Item = &'a Item<'b>> {
        header.iter().chain(self.inner.iter())
    }
}

/// Serializes the collected output as a k:v map
pub struct AsMapSerialize<I> {
    // Need RefCell due to the serialize receiver
    // taking an immutable ref
    inner: std::cell::RefCell<I>,
}

impl<'out, I> AsMapSerialize<I>
where
    I: Iterator<Item = &'out Item<'out>>,
{
    pub fn new(iter: I) -> Self {
        Self {
            inner: std::cell::RefCell::new(iter),
        }
    }
}

impl<'out, I> Serialize for AsMapSerialize<I>
where
    I: Iterator<Item = &'out Item<'out>>,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        let mut iter = self.inner.borrow_mut();
        while let Some(item) = iter.next() {
            map.serialize_entry(&item.as_marker(), &item)?;
        }
        map.end()
    }
}

/// Local representation of any possible valid output.
// Currently using an enum due to the low number of variants.
// If this enum gets larger than ~24 variants, should consider
// moving to a trait based implementation
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Item<'out> {
    Version(u32),
    Tag(Directive),
    Time(i64),
    Id(Arc<str>),
    Pid(u32),
    Data(&'out [u8]),
}

impl Marker for Item<'_> {
    type Marker = TagMarker;

    fn as_marker(&self) -> Self::Marker {
        match self {
            Self::Version(_) => TagMarker::Version,
            Self::Tag(_) => TagMarker::DataContext,
            Self::Time(_) => TagMarker::Time,
            Self::Id(_) => TagMarker::Id,
            Self::Pid(_) => TagMarker::Pid,
            Self::Data(_) => TagMarker::Data,
        }
    }
}
