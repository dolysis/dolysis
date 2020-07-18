use {
    crate::prelude::*,
    arrayvec::ArrayVec,
    lib_transport::{Common, Data, DataContext, Header, Record},
    std::{fmt, sync::Arc},
};

/// Local representation of DataContext
#[derive(Debug, Clone, Copy)]
pub enum Directive {
    Start,
    Stdout,
    Stderr,
    End,
}

impl Into<DataContext> for Directive {
    fn into(self) -> DataContext {
        match self {
            Self::Start => DataContext::Start,
            Self::Stdout => DataContext::Stdout,
            Self::Stderr => DataContext::Stderr,
            Self::End => DataContext::End,
        }
    }
}

impl std::fmt::Display for Directive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let x = match self {
            Self::Start => "BEGIN",
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
            Self::Start => "Start",
            Self::Stdout => "Stdout",
            Self::Stderr => "Stderr",
            Self::End => "End",
        };

        write!(f, "{}", x)
    }
}

/// Container for various relevant data that should be passed to the parser
#[derive(Debug, Default)]
pub struct OutputContext {
    inner: ArrayVec<[CxtItem; 3]>,
}

impl OutputContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_id(&mut self, id: &str) {
        self.inner.push(CxtItem::Id(Arc::from(id)))
    }

    pub fn insert_pid(&mut self, pid: u32) {
        self.inner.push(CxtItem::Pid(pid))
    }

    pub fn insert_version(&mut self, version: u32) {
        self.inner.push(CxtItem::Version(version))
    }

    fn items(&self) -> &[CxtItem] {
        &self.inner
    }
}

/// Local representation of context that is gathered early.
// Currently using an enum due to the low number of variants.
// If this enum gets larger than ~24 variants, should consider
// moving to a trait based implementation
#[derive(Debug, Clone)]
enum CxtItem {
    Version(u32),
    Id(Arc<str>),
    Pid(u32),
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
        cxt.map_or_else(Self::default, |cxt| cxt.into())
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
                CxtItem::Version(i) => {
                    state.version.replace(*i);
                    state
                }
                CxtItem::Id(i) => {
                    state.id.replace(i);
                    state
                }
                CxtItem::Pid(i) => {
                    state.pid.replace(*i);
                    state
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
        cxt.map_or_else(Self::default, |cxt| cxt.into())
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
                CxtItem::Version(i) => {
                    state.version.replace(*i);
                    state
                }
                CxtItem::Id(i) => {
                    state.id.replace(i.as_ref());
                    state
                }
                CxtItem::Pid(i) => {
                    state.pid.replace(*i);
                    state
                }
            })
    }
}
