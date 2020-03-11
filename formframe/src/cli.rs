#![allow(deprecated)]
use {
    crate::{
        error::{CfgErrSubject as Subject, ConfigError},
        load::filter::{FilterSet, JoinSet},
        prelude::{CrateResult as Result, *},
    },
    clap::{crate_authors, crate_version, App, Arg, ArgSettings},
    std::{
        fs::File,
        io::{Read, Seek, SeekFrom},
        path::Path,
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
                //.required(true)
                .validator(|s| Some(s.as_str()).filter(|s| (*s == "-") || Path::new(s).exists()).map(|_| ()).ok_or_else(|| format!("'{}' does not exist or is an invalid path", &s)))
                .help("File to read as input, - for stdin")
        )
}

pub struct ProgramArgs {
    filter: FilterSet,
    join: JoinSet,
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

        //let mut filter = DataInit::Filter(None);
        //let mut join = DataInit::Join(None);

        let mut filter: Option<Result<FilterSet>> = None;
        let mut join: Option<Result<JoinSet>> = None;

        store.values_of("config-file").map(|iter| {
            enter!(span, debug_span!("cfg.load", file = field::Empty));
            // We allow the user to specify multiple files with a requirement that somewhere in
            // these files are all the required config options. Which means that if we can't open a file,
            // or if the file is invalid yaml we shouldn't give up because other files may contain the
            // information we need
            iter.map(|s| {
                span.record("file", &s);
                File::open(s)
            })
            .try_for_each(|file_r| {
                file_r
                    .map_err(|e| e.into())
                    .and_then(|ref mut file| {
                        // Check current file for a FilterSet
                        let f = FilterSet::new_filter(file.by_ref())
                            .map_err(|e| ConfigError::Other(e).into())
                            .log(Level::DEBUG);
                        lift_result(f, &mut filter)?;

                        file.seek(SeekFrom::Start(0))?;

                        // Check current file for a JoinSet
                        let j = JoinSet::new_filter(file)
                            .map_err(|e| ConfigError::Other(e).into())
                            .log(Level::DEBUG);
                        lift_result(j, &mut join)?;

                        Ok(())
                    })
                    .log(Level::WARN)
            })
        });

        // Check to make sure we have all the required information
        let filter = filter.transpose().log(Level::ERROR)?;
        let join = join.transpose().log(Level::ERROR)?;

        filter
            .ok_or(ConfigError::Missing(Subject::Filter).into())
            .and_then(|filter| {
                join.ok_or(ConfigError::Missing(Subject::Filter).into())
                    .map(|join| (filter, join))
            })
            .map(|(filter, join)| Self { filter, join })
            .log(Level::ERROR)
    }

    pub fn get_filter(&self) -> &FilterSet {
        &self.filter
    }

    pub fn get_join(&self) -> &JoinSet {
        &self.join
    }

    // TODO: replace with user arg when implementing tcp/unix subcommand
    pub fn bind_addr(&self) -> &str {
        "127.0.0.1:8080"
    }
}

impl Into<Subject> for FilterSet {
    fn into(self) -> Subject {
        Subject::Filter
    }
}

impl Into<Subject> for JoinSet {
    fn into(self) -> Subject {
        Subject::Join
    }
}

fn lift_result<T>(mut cur: Result<T>, prev: &mut Option<Result<T>>) -> Result<()>
where
    T: Into<Subject>,
{
    use std::mem::swap;
    match prev {
        None => *prev = Some(cur),
        Some(prev) => match (cur.is_ok(), prev.is_ok()) {
            (true, false) | (false, false) => swap(&mut cur, prev),
            (true, true) => Err(ConfigError::Duplicate(cur.ok().take().unwrap().into()))?,
            (false, true) => (),
        },
    }

    Ok(())
}
