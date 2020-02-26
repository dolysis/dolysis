use {
    crate::{models::SpanDisplay, prelude::error},
    serde_yaml::Error as YamlError,
    std::{error, fmt},
    thiserror::Error,
};

#[derive(Debug)]
pub struct LoadError {
    inner: Box<Err>,
}

impl<E> From<E> for LoadError
where
    E: Into<Err>,
{
    fn from(err: E) -> Self {
        Self {
            inner: Box::new(err.into()),
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl error::Error for LoadError {}

#[derive(Debug, Error)]
pub enum Err {
    #[error("Duplicate root node name: {}, each root must have a unique name", .0)]
    DuplicateRootName(String),
    #[error("Failed to deserialize yaml: {}", .source)]
    YamlError {
        #[from]
        source: YamlError,
    },
}

impl Err {
    fn categorize(&self) -> Category {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Category {
    Yaml,
    FilterSyntax,
}

impl From<&Err> for Category {
    fn from(err: &Err) -> Self {
        match err {
            Err::YamlError { .. } => Self::Yaml,
            Err::DuplicateRootName { .. } => Self::FilterSyntax,
        }
    }
}

impl SpanDisplay for Category {
    fn span_output(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml => write!(f, "Yaml"),
            Self::FilterSyntax => write!(f, "FilterSyntax"),
        }
    }
}
