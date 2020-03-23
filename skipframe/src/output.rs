use {
    crate::prelude::*,
    serde::Serialize,
    serde_interface::{Common, Data, DataContext, Header, Marker, Record, TagMarker},
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

    pub fn items(&self) -> &[Item] {
        &self.inner
    }

    // pub fn stream<'a, 'b: 'a>(
    //     &'b self,
    //     header: &'a [Item<'b>],
    // ) -> impl Iterator<Item = &'a Item<'b>> {
    //     header.iter().chain(self.inner.iter())
    // }
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

#[derive(Debug, Default)]
pub struct HeaderBuilder<'ctx> {
    version: Option<u32>,
    tag: Option<DataContext>,
    time: Option<i64>,
    id: Option<&'ctx str>,
    pid: Option<u32>,
}

impl<'ctx> HeaderBuilder<'ctx> {
    pub fn new(cxt: Option<&'ctx OutputContext>) -> Self {
        cxt.map_or_else(|| Self::default(), |cxt| cxt.into())
    }

    pub fn is_done(&self) -> bool {
        self.version.is_some()
            && self.tag.is_some()
            && self.time.is_some()
            && self.id.is_some()
            && self.pid.is_some()
    }

    pub fn map<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        f(&mut self);
        self
    }

    pub fn and<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self),
    {
        f(self);
        self
    }

    pub fn tag<T>(&mut self, tag: T)
    where
        T: Into<DataContext>,
    {
        self.tag.replace(tag.into());
    }

    pub fn time(&mut self, time: i64) {
        self.time.replace(time);
    }

    pub fn done_unchecked(self) -> Record<'ctx, 'static> {
        if !self.is_done() {
            panic!("Attempted to convert an incomplete HeaderBuilder to a Record")
        } else {
            let header = Header {
                required: Common::new(self.version.unwrap()),
                time: self.time.unwrap(),
                id: self.id.map(|id| id.into()).unwrap(),
                pid: self.pid.unwrap(),
                cxt: self.tag.unwrap(),
            };

            Record::Header(header)
        }
    }
}

impl<'ctx> From<&'ctx OutputContext> for HeaderBuilder<'ctx> {
    fn from(base: &'ctx OutputContext) -> Self {
        base.items()
            .iter()
            .fold(Self::default(), |mut state, item| match item {
                Item::Version(i) => {
                    state.version.replace(*i);
                    state
                }
                Item::Tag(i) => {
                    state.tag.replace((*i).into());
                    state
                }
                Item::Time(i) => {
                    state.time.replace(*i);
                    state
                }
                Item::Id(i) => {
                    state.id.replace(i);
                    state
                }
                Item::Pid(i) => {
                    state.pid.replace(*i);
                    state
                }
                Item::Data(_) => {
                    unreachable!("Not possible for OutputContext to hold non static refs")
                }
            })
    }
}

#[derive(Debug, Default)]
pub struct DataBuilder<'ctx, 'out> {
    version: Option<u32>,
    tag: Option<DataContext>,
    time: Option<i64>,
    id: Option<&'ctx str>,
    pid: Option<u32>,
    data: Option<&'out str>,
}

impl<'ctx, 'out> DataBuilder<'ctx, 'out> {
    pub fn new(cxt: Option<&'ctx OutputContext>) -> Self {
        cxt.map_or_else(|| Self::default(), |cxt| cxt.into())
    }

    pub fn map<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        f(&mut self);
        self
    }

    pub fn and<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut Self),
    {
        f(self);
        self
    }

    pub fn tag<T>(&mut self, tag: T)
    where
        T: Into<DataContext>,
    {
        self.tag.replace(tag.into());
    }

    pub fn time(&mut self, time: i64) {
        self.time.replace(time);
    }

    pub fn data(&mut self, data: &'out str) {
        self.data.replace(data);
    }

    pub fn is_done(&self) -> bool {
        self.version.is_some()
            && self.tag.is_some()
            && self.time.is_some()
            && self.id.is_some()
            && self.pid.is_some()
            && self.data.is_some()
    }

    pub fn done_unchecked(self) -> Record<'ctx, 'out> {
        if !self.is_done() {
            panic!("Attempted to convert an incomplete DataBuilder to a Record")
        } else {
            let data = Data {
                required: Common::new(self.version.unwrap()),
                time: self.time.unwrap(),
                id: self.id.map(|id| id.into()).unwrap(),
                pid: self.pid.unwrap(),
                cxt: self.tag.unwrap(),
                data: self.data.map(|d| d.into()).unwrap(),
            };

            Record::Data(data)
        }
    }
}

impl<'ctx> From<&'ctx OutputContext> for DataBuilder<'ctx, '_> {
    fn from(base: &'ctx OutputContext) -> Self {
        base.items()
            .iter()
            .fold(Self::default(), |mut state, item| match item {
                Item::Version(i) => {
                    state.version.replace(*i);
                    state
                }
                Item::Tag(i) => {
                    state.tag.replace((*i).into());
                    state
                }
                Item::Time(i) => {
                    state.time.replace(*i);
                    state
                }
                Item::Id(i) => {
                    state.id.replace(i.as_ref());
                    state
                }
                Item::Pid(i) => {
                    state.pid.replace(*i);
                    state
                }
                Item::Data(_) => {
                    unreachable!("Not possible for OutputContext to hold non static refs")
                }
            })
    }
}
