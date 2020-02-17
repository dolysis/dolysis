use {
    crate::load::error::LoadError,
    std::{fmt, io::Error as IoError},
    thiserror::Error,
};

pub type Result<T> = std::result::Result<T, CrateError>;

#[derive(Debug)]
pub struct CrateError {
    inner: BoxOrStat,
}

#[derive(Debug)]
enum BoxOrStat {
    Box(Box<Err>),
    Static(&'static CrateError),
}

impl From<&'static CrateError> for CrateError {
    fn from(r: &'static CrateError) -> Self {
        Self {
            inner: BoxOrStat::Static(r),
        }
    }
}

impl<E> From<E> for CrateError
where
    E: Into<Err>,
{
    fn from(err: E) -> Self {
        Self {
            inner: BoxOrStat::Box(Box::new(err.into())),
        }
    }
}

impl fmt::Display for CrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner {
            BoxOrStat::Box(ref e) => write!(f, "{}", e),
            BoxOrStat::Static(r) => write!(f, "{}", r),
        }
    }
}

#[derive(Debug, Error)]
pub enum Err {
    #[error("IO error: {}", .source)]
    Io {
        #[from]
        source: IoError,
    },
    #[error("Invalid config, no '{}' mapping was found", .source)]
    InvalidConfig {
        #[from]
        source: ConfigError,
    },
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
