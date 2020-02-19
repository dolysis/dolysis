use {
    crate::{
        load::error::LoadError,
        models::SpanDisplay,
        prelude::{debug, error, info, trace, warn},
    },
    std::{error, fmt, io::Error as IoError},
    thiserror::Error,
};

pub type CrateResult<T> = std::result::Result<T, CrateError>;
pub type MainResult<T> = std::result::Result<T, RefError>;

// #[macro_export]
// macro_rules! elog {
//     (none, $err:expr) => {{
//         use crate::error::NoLog;
//         NoLog::from($err)
//     }};
//     (trace, $err:expr) => {
//         ($err, Some(tracing::Level::TRACE)).into()
//     };
//     (debug, $err:expr) => {
//         ($err, Some(tracing::Level::DEBUG)).into()
//     };
//     (info, $err:expr) => {
//         ($err, Some(tracing::Level::INFO)).into()
//     };
//     (warn, $err:expr) => {
//         ($err, Some(tracing::Level::WARN)).into()
//     };
//     (error, $err:expr) => {
//         ($err, Some(tracing::Level::ERROR)).into()
//     };
// }

#[derive(Debug)]
pub struct CrateError {
    inner: Box<Err>,
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
}

impl From<&Err> for Category {
    fn from(err: &Err) -> Self {
        match err {
            Err::Io { .. } => Self::Io,
            Err::InvalidConfig { .. } => Self::Config,
        }
    }
}

impl SpanDisplay for Category {
    fn span_output(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io => write!(f, "IO"),
            Self::Config => write!(f, "Config"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing config: {}", .0)]
    Missing(CfgErrSubject),
    #[error("duplicate config: {}", .0)]
    Duplicate(CfgErrSubject),
    #[error(transparent)]
    Other(LoadError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CfgErrSubject {
    Filter,
    Map,
    Transform,
}

impl fmt::Display for CfgErrSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = match self {
            Self::Filter => format_args!("filter"),
            Self::Map => format_args!("map"),
            Self::Transform => format_args!("transform"),
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
