use {
    crossbeam_channel::SendError,
    futures::channel::mpsc::SendError as AsyncSendError,
    serde_interface::InterfaceError,
    std::{ffi::OsString, fmt, io::Error as IoError, num::ParseIntError, str::Utf8Error},
    thiserror::Error,
    walkdir::Error as WalkdirError,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error {
    time: i64,
    inner: Err,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for Error {}

impl Into<InterfaceError> for Error {
    fn into(self) -> InterfaceError {
        let time = self.time;
        self.inner.to_interface_err(time)
    }
}

impl<F> From<F> for Error
where
    F: Into<Err>,
{
    fn from(f: F) -> Self {
        Self {
            time: chrono::Utc::now().timestamp_nanos(),
            inner: f.into(),
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
    pub fn to_interface_err(self, time: i64) -> InterfaceError {
        InterfaceError::new(time, None, self)
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
