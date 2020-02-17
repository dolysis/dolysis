#![allow(deprecated)]
use {
    crate::load::filter::FilterSet,
    clap::{crate_authors, crate_version, App, Arg, ArgSettings},
    std::{
        error,
        fs::File,
        path::{Path, PathBuf},
        sync::Arc,
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

    pub fn try_init(cli: App<'_, '_>) -> Result<Self, Arc<dyn error::Error + Send + Sync>> {
        Self::__try_init(cli).map_err(|b| b.into())
    }

    fn __try_init(cli: App<'_, '_>) -> Result<Self, Box<dyn error::Error + Send + Sync>> {
        let store = cli.get_matches();

        let input_type = store
            .value_of("debug-input")
            .map(|s| match s {
                "-" => DebugInputKind::Stdin,
                path => DebugInputKind::File(PathBuf::from(path)),
            })
            .unwrap();

        let mut filter = None;

        store.values_of("config-file").map(|iter| {
            // We allow the user to specify multiple files with a requirement that somewhere in
            // these files are all the required config options. Which means that if we can't open a file,
            // or if the file is invalid yaml we shouldn't give up because other files may contain the
            // information we need
            iter.map(|s| File::open(s)).try_for_each(|res| match res {
                Ok(f) => {
                    FilterSet::try_new(f).and_then(|fset| checked_set(&mut filter, fset))
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
        if filter.is_some() {
            Ok(Self {
                filter: filter.unwrap(),
                input_type,
            })
        } else {
            Err(format!("Missing mandatory config information...").into())
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

fn checked_set<T>(store: &mut Option<T>, value: T) -> Result<(), Box<dyn error::Error>> {
    if store.is_none() {
        *store = Some(value);
        Ok(())
    } else {
        Err(format!("Duplicate config value").into())
    }
}

enum DebugInputKind {
    Stdin,
    File(PathBuf),
}
