use {
    crate::{load::error::LoadError, models::SpanDisplay, prelude::*},
    lib_transport::DataContext as RecordContext,
    std::{error, fmt, io::Error as IoError, string::FromUtf8Error},
    thiserror::Error,
};

pub type CrateResult<T> = std::result::Result<T, CrateError>;
pub type MainResult<T> = std::result::Result<T, RefError>;

#[derive(Debug)]
pub struct CrateError {
    inner: Box<Err>,
}

impl CrateError {
    pub fn err_invalid_record<T>(invalid: RecordContext, expected: T) -> Self
    where
        T: AsRef<[RecordContext]>,
    {
        Err::InvalidRecordContext {
            invalid,
            expected: rcxt_join(expected),
        }
        .into()
    }
}

impl<E> From<E> for CrateError
where
    E: Into<Err>,
{
    fn from(err: E) -> Self {
        Self {
            inner: Box::new(err.into()),
        }
    }
}

impl fmt::Display for CrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl error::Error for CrateError {}

/// Abstraction layer for potential early return in main if ProgramArgs init failed
#[derive(Debug)]
pub struct RefError {
    ref_err: Or,
}

impl From<&'static CrateError> for RefError {
    fn from(r: &'static CrateError) -> Self {
        Self {
            ref_err: Or::Ref(r),
        }
    }
}

impl From<CrateError> for RefError {
    fn from(e: CrateError) -> Self {
        Self {
            ref_err: Or::Err(e),
        }
    }
}

impl AsRef<CrateError> for RefError {
    fn as_ref(&self) -> &CrateError {
        match self.ref_err {
            Or::Ref(r) => r,
            Or::Err(ref e) => e,
        }
    }
}

impl fmt::Display for RefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

impl error::Error for RefError {}

#[derive(Debug)]
enum Or {
    Ref(&'static CrateError),
    Err(CrateError),
}

#[derive(Debug, Error)]
pub enum Err {
    #[error("IO error: {}", .source)]
    Io {
        #[from]
        source: IoError,
    },
    #[error("Invalid config, {}", .source)]
    InvalidConfig {
        #[from]
        source: ConfigError,
    },
    #[error("Bad record context, expected '{}' got '{}'", .expected, rcxt_display(.invalid))]
    InvalidRecordContext {
        invalid: RecordContext,
        expected: String,
    },
    #[error("Record data is not valid UTF8: {}", .source)]
    RecordDataInvalidUTF8 {
        #[from]
        source: FromUtf8Error,
    },
}

impl Err {
    fn categorize(&self) -> Category {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Category {
    Io,
    Config,
    Record,
}

impl From<&Err> for Category {
    fn from(err: &Err) -> Self {
        match err {
            Err::Io { .. } => Self::Io,
            Err::InvalidConfig { .. } => Self::Config,
            Err::InvalidRecordContext { .. } | Err::RecordDataInvalidUTF8 { .. } => Self::Record,
        }
    }
}

impl SpanDisplay for Category {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Io => "IO",
            Self::Config => "Config",
            Self::Record => "Record",
        };

        write!(f, "{}", s)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing config: {}", .0)]
    Missing(CfgErrSubject),
    #[error("duplicate config: {}", .0)]
    Duplicate(CfgErrSubject),
    #[error("key '{}' not found in: {}", .1, .0)]
    InvalidExecKey(CfgErrSubject, String),
    #[error(transparent)]
    Other(LoadError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CfgErrSubject {
    Filter,
    Join,
    Map,
    Transform,
    Exec,
    Load,
}

impl fmt::Display for CfgErrSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = match self {
            Self::Filter => format_args!("filter"),
            Self::Join => format_args!("join"),
            Self::Map => format_args!("map"),
            Self::Transform => format_args!("transform"),
            Self::Exec => format_args!("exec"),
            Self::Load => format_args!("load"),
        };

        write!(f, "{}", o)
    }
}

pub trait LogError {
    //type RetVal;
    fn log(self, level: tracing::Level) -> Self;
}

impl<T> LogError for CrateResult<T> {
    fn log(self, level: tracing::Level) -> Self {
        match self {
            ok @ Ok(_) => ok,
            Err(e) => Err(e.log(level)),
        }
    }
}

impl LogError for CrateError {
    fn log(self, level: tracing::Level) -> Self {
        match level {
            tracing::Level::ERROR => {
                error!(kind = %self.inner.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::WARN => {
                warn!(kind = %self.inner.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::INFO => {
                info!(kind = %self.inner.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::DEBUG => {
                debug!(kind = %self.inner.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::TRACE => {
                trace!(kind = %self.inner.categorize().span_display(), message = %self.inner)
            }
        }
        self
    }
}

fn rcxt_display(cxt: &RecordContext) -> &str {
    match cxt {
        RecordContext::Start => "Start",
        RecordContext::End => "End",
        RecordContext::Stdout => "Stdout",
        RecordContext::Stderr => "Stderr",
    }
}

fn rcxt_join<T>(valid: T) -> String
where
    T: AsRef<[RecordContext]>,
{
    valid.as_ref().iter().identify_first_last().fold(
        String::default(),
        |mut out, (_, last, cxt)| {
            match (cxt, last) {
                (cxt, false) => out.extend([rcxt_display(cxt), ","].iter().copied()),
                (cxt, true) => out.extend([rcxt_display(cxt)].iter().copied()),
            };

            out
        },
    )
}
