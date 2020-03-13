use {
    super::filters::JoinSet,
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
    #[error("{}", JiiDisplay(*.0))]
    JoinInvalidInput((bool, bool, bool)),
    #[error("Failed to deserialize yaml: {}", .source)]
    YamlError {
        #[from]
        source: YamlError,
    },
}

impl From<(bool, bool, bool)> for Err {
    fn from(t: (bool, bool, bool)) -> Self {
        Err::JoinInvalidInput(t)
    }
}

impl Err {
    // fn categorize(&self) -> Category {
    //     self.into()
    // }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Category {
    Yaml,
    FilterSyntax,
    JoinSyntax,
}

impl From<&Err> for Category {
    fn from(err: &Err) -> Self {
        match err {
            Err::YamlError { .. } => Self::Yaml,
            Err::DuplicateRootName { .. } => Self::FilterSyntax,
            Err::JoinInvalidInput(_) => Self::JoinSyntax,
        }
    }
}

impl SpanDisplay for Category {
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Yaml => write!(f, "Yaml"),
            Self::FilterSyntax => write!(f, "FilterSyntax"),
            Self::JoinSyntax => write!(f, "JoinSyntax"),
        }
    }
}

#[derive(Debug)]
struct JiiDisplay((bool, bool, bool));

impl JiiDisplay {
    fn print_valid_input(f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut valid = JoinSet::VALID_INPUT_KINDS.iter().peekable();

        write!(f, "[")?;
        loop {
            let item = valid.next();
            let last = valid.peek().is_none();

            match (item, last) {
                (Some(input), false) => write!(
                    f,
                    "({}, {}, {}), ",
                    bp(input.0, "Start"),
                    bp(input.1, "While"),
                    bp(input.2, "End")
                )?,
                (Some(input), true) => write!(
                    f,
                    "({}, {}, {})",
                    bp(input.0, "Start"),
                    bp(input.1, "While"),
                    bp(input.2, "End")
                )?,
                (None, _) => break,
            }
        }
        write!(f, "]")
    }
}

impl fmt::Display for JiiDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (start, cont, end) = self.0;
        write!(f, "Invalid Join input, requires one of: ")?;
        Self::print_valid_input(f)?;
        write!(
            f,
            ", got: ({}, {}, {})",
            bp(start, "Start"),
            bp(cont, "While"),
            bp(end, "End"),
        )
    }
}

impl error::Error for JiiDisplay {}

fn bp(descrim: bool, a: &str) -> &str {
    if descrim {
        a
    } else {
        "-"
    }
}
