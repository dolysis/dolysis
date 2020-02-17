use {
    serde::ser::{SerializeMap, Serializer},
    //crate::prelude::*,
    serde::Serialize,
    serde_interface::{DataContext, Marker, TagMarker},
    std::sync::Arc,
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

    pub fn as_ref(&self) -> &[Item] {
        &self.inner
    }

    pub fn stream<'a, 'b: 'a>(
        &'b self,
        header: &'a [Item<'b>],
    ) -> impl Iterator<Item = &'a Item<'b>> {
        header.iter().chain(self.inner.iter())
    }
}

/// Short lived glue struct for processing child process's output streams
/// Note that 'out refers to the short lived byte slice containing the child output,
/// whereas 'a refers to a lifetime _at least_ as long as the backing OutputContext's
/// lifetime, but may be longer
pub struct RefContext<'a, 'out> {
    backing: &'a [Item<'a>],
    data: Item<'out>,
}

impl<'a, 'out> RefContext<'a, 'out> {
    pub fn new(backing: &'a OutputContext, data: &'out [u8]) -> Self {
        Self {
            backing: backing.as_ref(),
            data: Item::Data(data),
        }
    }

    pub fn stream(
        &'out self,
        header: &'out [Item<'static>],
    ) -> impl Iterator<Item = &'out Item<'out>> {
        header
            .iter()
            .chain(self.backing)
            .map(|i| Some(i))
            .chain(Some(&self.data).map(|d| Some(d)))
            .filter_map(|o| o)
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
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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
