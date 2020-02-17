#![allow(deprecated)]
use {
    crate::{
        error::{CfgErrSubject as Subject, ConfigError},
        load::filter::FilterSet,
        models::SpanDisplay,
        prelude::{CrateResult as Result, *},
    },
    clap::{crate_authors, crate_version, App, Arg, ArgSettings},
    std::{
        fs::File,
        path::{Path, PathBuf},
    },
};

pub fn generate_cli<'a, 'b>() -> App<'a, 'b> {
    App::new("skipframe")
        .about("This program transforms input streams")
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .arg(
            Arg::with_name("config-file")
                .short("f")
                .long("file")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .value_name("PATH")
                .validator(|s| Some(s.as_str()).filter(|s| Path::new(s).exists()).map(|_| ()).ok_or_else(|| format!("'{}' does not exist or is an invalid path", s)))
                .help("Read a config file, can be called multiple times (--help for more information)")
                .long_help("Read a config file, can be called multiple times. This program requires 'filter', 'map' \
                            'transform' and 'execute' objects, along with an optional 'config' object. These do not \
                            need to be stored in the same file, but each file needs to be valid .yaml and each object \
                            should be passed only once.")
        )
        .arg(
            Arg::with_name("debug-input")
                .long("input")
                .takes_value(true)
                .value_name("PATH")
                .set(ArgSettings::AllowLeadingHyphen)
                .required(true)
                .validator(|s| Some(s.as_str()).filter(|s| (*s == "-") || Path::new(s).exists()).map(|_| ()).ok_or_else(|| format!("'{}' does not exist or is an invalid path", &s)))
                .help("File to read as input, - for stdin")
        )
}

pub struct ProgramArgs {
    filter: FilterSet,
    input_type: DebugInputKind,
}

impl ProgramArgs {
    pub unsafe fn init_unchecked(cli: App<'_, '_>) -> Self {
        Self::try_init(cli).unwrap()
    }

    pub fn try_init(cli: App<'_, '_>) -> Result<Self> {
        enter!(always_span!("init.cli"));
        Self::__try_init(cli)
    }

    fn __try_init(cli: App<'_, '_>) -> Result<Self> {
        let store = cli.get_matches();

        let input_type = store
            .value_of("debug-input")
            .map(|s| match s {
                "-" => DebugInputKind::Stdin,
                path => DebugInputKind::File(PathBuf::from(path)),
            })
            .unwrap();

        trace!(source = %input_type.span_display(), "Reading input from...");

        let mut filter = DataInit::Filter(None);

        store.values_of("config-file").map(|iter| {
            // We allow the user to specify multiple files with a requirement that somewhere in
            // these files are all the required config options. Which means that if we can't open a file,
            // or if the file is invalid yaml we shouldn't give up because other files may contain the
            // information we need
            iter.map(|s| {
                enter!(debug_span!("load config", file = s));
                File::open(s)
            })
            .try_for_each(|res| match res {
                Ok(f) => {
                    FilterSet::try_new(f)
                        .map_err(|e| ConfigError::Other(e).into())
                        .and_then(|fset| filter.checked_set(DataInit::from(fset)))
                    // MapSet::try_new()
                    // TransformSet::try_new()
                    // etc...
                }
                // TODO: Once logging implemented log e
                Err(_e) => Ok(()),
            })
        });

        // When we implement more objects it will be filter.and(...).and(...)...is_some()
        // Check to make sure we have all the required information
        if filter.is_set() {
            Ok(Self {
                filter: filter.into_filter().unwrap(),
                input_type,
            })
        } else {
            Err(ConfigError::Missing(Subject::Filter).into())
        }
    }

    pub fn get_filter(&self) -> &FilterSet {
        &self.filter
    }

    pub fn get_input(&self) -> Option<&Path> {
        match self.input_type {
            DebugInputKind::Stdin => None,
            DebugInputKind::File(ref p) => Some(p.as_ref()),
        }
    }
}

#[derive(Debug)]
enum DebugInputKind {
    Stdin,
    File(PathBuf),
}

impl SpanDisplay for DebugInputKind {
    fn span_output(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdin => write!(f, "stdin"),
            Self::File(path) => write!(f, "{}", path.display()),
        }
    }
}

#[derive(Debug)]
enum DataInit {
    Filter(Option<FilterSet>),
}

impl From<FilterSet> for DataInit {
    fn from(set: FilterSet) -> Self {
        Self::Filter(Some(set))
    }
}

impl Into<Subject> for &DataInit {
    fn into(self) -> Subject {
        match self {
            DataInit::Filter(_) => Subject::Filter,
        }
    }
}

impl DataInit {
    fn and(&self, other: Self) -> Option<()> {
        match (self.is_set(), other.is_set()) {
            (true, true) => Some(()),
            (_, _) => None,
        }
    }

    fn is_set(&self) -> bool {
        !self.is_empty()
    }

    fn is_empty(&self) -> bool {
        match self {
            DataInit::Filter(o) => o.is_none(),
        }
    }

    fn checked_set<T>(&mut self, value: T) -> Result<()>
    where
        T: Into<DataInit>,
    {
        if self.is_empty() {
            *self = value.into();
            Ok(())
        } else {
            // Lotta intos: T -> DataInit -> &DataInit -> Subject -> CrateError
            Err(ConfigError::Duplicate((&value.into()).into()).into())
        }
    }
    fn into_filter(self) -> Option<FilterSet> {
        match self {
            Self::Filter(o) => o,
            //_ => None,
        }
    }
}
