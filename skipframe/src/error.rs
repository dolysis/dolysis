use {
    crate::{models::SpanDisplay, prelude::*},
    crossbeam_channel::SendError,
    futures::channel::mpsc::SendError as AsyncSendError,
    std::{ffi::OsString, fmt, io::Error as IoError, num::ParseIntError, str::Utf8Error},
    thiserror::Error,
    walkdir::Error as WalkdirError,
};

pub type CrateResult<T> = std::result::Result<T, CrateError>;

#[derive(Debug)]
pub struct CrateError {
    inner: Box<Err>,
}

impl CrateError {
    pub fn categorize(&self) -> Category {
        self.inner.categorize()
    }
}

impl fmt::Display for CrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for CrateError {}

impl<F> From<F> for CrateError
where
    F: Into<Err>,
{
    fn from(f: F) -> Self {
        Self {
            inner: Box::new(f.into()),
        }
    }
}

#[derive(Debug, Error)]
pub enum Err {
    #[error("Non-UTF8 paths are not supported ({})", .0.to_string_lossy())]
    PathInvalidUTF8(OsString),
    #[error("{}", .source)]
    PathError {
        #[from]
        source: WalkdirError,
    },
    #[error("Invalid priority order: {}", .source)]
    PathPriorityParse {
        #[from]
        source: ParseIntError,
    },
    #[error("{}", .source)]
    Io {
        #[from]
        source: IoError,
    },
    #[error("Invalid output: {}", .source)]
    Utf8 {
        #[from]
        source: Utf8Error,
    },
    #[error("Async channel send error: {}", .source)]
    AsyncSendError {
        #[from]
        source: AsyncSendError,
    },
    #[error("Channel Receiver closed unexpectedly")]
    SendError,
}

impl Err {
    pub fn categorize(&self) -> Category {
        match self {
            Self::PathInvalidUTF8(_) | Self::PathError { .. } | Self::PathPriorityParse { .. } => {
                Category::FilePath
            }
            Self::Io { .. } => Category::Io,
            Self::Utf8 { .. } => Category::Utf8,
            Self::AsyncSendError { .. } | Self::SendError => Category::ChannelError,
        }
    }
}

impl<T> From<SendError<T>> for Err {
    fn from(_err: SendError<T>) -> Self {
        Self::SendError
    }
}

impl From<OsString> for Err {
    fn from(name: OsString) -> Self {
        Self::PathInvalidUTF8(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Category {
    FilePath,
    Io,
    ChannelError,
    Utf8,
}

impl SpanDisplay for Category {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let output = match self {
            Self::FilePath => "FilePath",
            Self::Io => "IO",
            Self::ChannelError => "ChannelError",
            Self::Utf8 => "UTF8",
        };

        write!(f, "{}", output)
    }
}

pub trait LogError {
    fn ref_log(&self, level: tracing::Level);

    fn log(self, level: tracing::Level) -> Self
    where
        Self: Sized,
    {
        (&self).ref_log(level);
        self
    }
}

impl<T> LogError for CrateResult<T> {
    fn ref_log(&self, level: tracing::Level) {
        match self {
            Ok(_) => (),
            Err(e) => e.ref_log(level),
        }
    }

    fn log(self, level: tracing::Level) -> Self
    where
        Self: Sized,
    {
        match self {
            ok @ Ok(_) => ok,
            Err(e) => Err(e.log(level)),
        }
    }
}

impl LogError for CrateError {
    fn ref_log(&self, level: tracing::Level) {
        match level {
            tracing::Level::ERROR => {
                error!(kind = %self.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::WARN => {
                warn!(kind = %self.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::INFO => {
                info!(kind = %self.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::DEBUG => {
                debug!(kind = %self.categorize().span_display(), message = %self.inner)
            }
            tracing::Level::TRACE => {
                trace!(kind = %self.categorize().span_display(), message = %self.inner)
            }
        }
    }
}
