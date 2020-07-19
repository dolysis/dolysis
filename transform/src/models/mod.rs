use {
    crate::{error::MainResult, prelude::*, ARGS},
    lib_transport::{
        Common, Data as RecordData, DataContext as RecordContext, Header as RecordHeader, Record,
    },
    std::{
        convert::{TryFrom, TryInto},
        fmt,
    },
    tracing_subscriber::{EnvFilter, FmtSubscriber},
};

pub mod tcp;

/// Initialize the global logger. This function must be called before ARGS is initialized,
/// otherwise logs generated during CLI parsing will be silently ignored
pub fn init_logging() {
    let root_subscriber = FmtSubscriber::builder()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::default().add_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        }))
        .with_filter_reloading()
        .finish();
    tracing::subscriber::set_global_default(root_subscriber).expect("Failed to init logging");
    info!("<== Logs Start ==>")
}

/// This function should be the first to deref ARGS,
/// giving the program a chance to bail if anything went wrong on initialization.
/// It is an invariant of this program that any call to ARGs after this call will never fail
pub fn check_args() -> MainResult<()> {
    let args = ARGS.as_ref();
    match args {
        Ok(_) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub trait ResultInspect {
    type Item;

    fn inspect<F>(self, f: F) -> Self
    where
        Self: Sized,
        F: FnMut(&Self::Item);
}

impl<T, E> ResultInspect for std::result::Result<T, E> {
    type Item = Self;

    fn inspect<F>(self, mut f: F) -> Self
    where
        Self: Sized,
        F: FnMut(&Self::Item),
    {
        f(&self);
        self
    }
}

pub trait SpanDisplay {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn span_display(&self) -> LocalDisplay<Self>
    where
        Self: Sized,
    {
        LocalDisplay::new(self)
    }
}

impl SpanDisplay for Record<'_, '_> {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Record::Header { .. } => "Header",
            Record::Data { .. } => "Data",
            Record::StreamStart => "StreamStart",
            Record::StreamEnd => "StreamEnd",
            Record::Log { .. } => "Log",
            Record::Error { .. } => "Error",
        };

        write!(f, "{}", s)
    }
}

pub struct LocalDisplay<'a, T> {
    owner: &'a T,
}

impl<'a, T> LocalDisplay<'a, T> {
    pub fn new(owner: &'a T) -> Self
    where
        T: SpanDisplay,
    {
        Self { owner }
    }
}

impl<'a, T> fmt::Display for LocalDisplay<'a, T>
where
    T: SpanDisplay,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.owner.span_print(f)
    }
}

/// Custom iterator interface for checking if an item
/// is the first or last item in an iterator
/// returns a tuple -> (is_first: bool, is_last: bool, item)
pub trait IdentifyFirstLast: Iterator + Sized {
    fn identify_first_last(self) -> FirstLast<Self>;
}

impl<I> IdentifyFirstLast for I
where
    I: Iterator,
{
    /// (is_first: bool, is_last: bool, item)
    fn identify_first_last(self) -> FirstLast<Self> {
        FirstLast(true, self.peekable())
    }
}

pub struct FirstLast<I>(bool, std::iter::Peekable<I>)
where
    I: Iterator;

impl<I> Iterator for FirstLast<I>
where
    I: Iterator,
{
    type Item = (bool, bool, I::Item);

    fn next(&mut self) -> Option<Self::Item> {
        let first = std::mem::replace(&mut self.0, false);
        self.1.next().map(|e| (first, self.1.peek().is_none(), e))
    }
}

#[derive(Debug)]
enum LocalRecord {
    Header(Header),
    Data(Data),
}

impl Into<Record<'static, 'static>> for LocalRecord {
    fn into(self) -> Record<'static, 'static> {
        match self {
            Self::Header(r) => r.into(),
            Self::Data(r) => r.into(),
        }
    }
}

impl<'i> TryFrom<RecordHeader<'i>> for LocalRecord {
    type Error = CrateError;

    fn try_from(value: RecordHeader) -> Result<Self, Self::Error> {
        Ok(Self::Header(value.try_into()?))
    }
}

impl<'i, 'd> TryFrom<RecordData<'i, 'd>> for LocalRecord {
    type Error = CrateError;

    fn try_from(value: RecordData) -> Result<Self, Self::Error> {
        Ok(Self::Data(value.try_into()?))
    }
}

#[derive(Debug)]
struct Header {
    pub version: u32,
    pub time: i64,
    pub id: String,
    pub pid: u32,
    pub cxt: HeaderContext,
}

impl<'i> TryFrom<RecordHeader<'i>> for Header {
    type Error = CrateError;

    fn try_from(value: RecordHeader) -> Result<Self, Self::Error> {
        Ok(Self {
            version: value.required.version,
            time: value.time,
            id: value.id.into(),
            pid: value.pid,
            cxt: HeaderContext::try_from(value.cxt)?,
        })
    }
}

impl Into<Record<'static, 'static>> for Header {
    fn into(self) -> Record<'static, 'static> {
        Record::Header(RecordHeader {
            required: Common {
                version: self.version,
            },
            time: self.time,
            id: self.id.into(),
            pid: self.pid,
            cxt: self.cxt.into(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum HeaderContext {
    Start,
    End,
}

impl HeaderContext {
    const VALID: [RecordContext; 2] = [RecordContext::Start, RecordContext::End];
}

impl Into<RecordContext> for HeaderContext {
    fn into(self) -> RecordContext {
        match self {
            Self::Start => RecordContext::Start,
            Self::End => RecordContext::End,
        }
    }
}

impl TryFrom<RecordContext> for HeaderContext {
    type Error = CrateError;

    fn try_from(value: RecordContext) -> Result<Self, Self::Error> {
        match value {
            RecordContext::Start => Ok(Self::Start),
            RecordContext::End => Ok(Self::End),
            invald => Err(CrateError::err_invalid_record(invald, &Self::VALID)),
        }
    }
}

#[derive(Debug)]
struct Data {
    pub version: u32,
    pub time: i64,
    pub id: String,
    pub pid: u32,
    pub cxt: DataContext,
    pub data: String,
}

impl<'i, 'd> TryFrom<RecordData<'i, 'd>> for Data {
    type Error = CrateError;

    fn try_from(value: RecordData) -> Result<Self, Self::Error> {
        Ok(Self {
            version: value.required.version,
            time: value.time,
            id: value.id.into(),
            pid: value.pid,
            cxt: DataContext::try_from(value.cxt)?,
            data: value.data.into(),
        })
    }
}

impl Into<Record<'static, 'static>> for Data {
    fn into(self) -> Record<'static, 'static> {
        Record::Data(RecordData {
            required: Common {
                version: self.version,
            },
            time: self.time,
            id: self.id.into(),
            pid: self.pid,
            cxt: self.cxt.into(),
            data: self.data.into(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DataContext {
    Stdout,
    Stderr,
}

impl DataContext {
    const VALID: [RecordContext; 2] = [RecordContext::Stdout, RecordContext::Stderr];
}

impl Into<RecordContext> for DataContext {
    fn into(self) -> RecordContext {
        match self {
            Self::Stdout => RecordContext::Stdout,
            Self::Stderr => RecordContext::Stderr,
        }
    }
}

impl TryFrom<RecordContext> for DataContext {
    type Error = CrateError;

    fn try_from(value: RecordContext) -> Result<Self, Self::Error> {
        match value {
            RecordContext::Stdout => Ok(Self::Stdout),
            RecordContext::Stderr => Ok(Self::Stderr),
            invald => Err(CrateError::err_invalid_record(invald, &Self::VALID)),
        }
    }
}
