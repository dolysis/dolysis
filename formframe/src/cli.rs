#![allow(deprecated)]
use {
    crate::{
        error::{CfgErrSubject as Subject, ConfigError},
        load::filter::{FilterSet, FilterWrap, JoinSet, JoinWrap},
        prelude::{CrateResult as Result, *},
    },
    clap::{crate_authors, crate_version, App, Arg, ArgSettings},
    serde::{Deserialize, Deserializer},
    serde_yaml::from_reader as read_yaml,
    std::{fs::File, path::Path},
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
    exec: Vec<Exec>,
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

        let mut filter: Option<Result<FilterSet>> = None;
        let mut join: Option<Result<JoinSet>> = None;
        let mut exec: Option<Result<Vec<Exec>>> = None;

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
                        // Deserialize current file
                        let ConfigDeserialize {
                            filter: f,
                            join: j,
                            exec: e,
                        } = read_yaml(file).unwrap();

                        // Check current file for a FilterSet
                        lift_result(f.map(|res| res.log(Level::DEBUG)), &mut filter)?;

                        // Check current file for a JoinSet
                        lift_result(j.map(|res| res.log(Level::DEBUG)), &mut join)?;

                        // Check current file for an Exec list
                        lift_result(e.map(Ok), &mut exec)?;

                        Ok(())
                    })
                    .log(Level::WARN)
            })
        });

        // Check to make sure we have all the required information
        let filter = filter
            .transpose()
            .and_then(|o| o.ok_or(ConfigError::Missing(Subject::Filter).into()))
            .log(Level::ERROR)?;
        let join = join
            .transpose()
            .and_then(|o| o.ok_or(ConfigError::Missing(Subject::Join).into()))
            .log(Level::ERROR)?;
        let exec = exec
            .transpose()
            .and_then(|o| o.ok_or(ConfigError::Missing(Subject::Join).into()))
            .and_then(|vec| {
                vec.iter()
                    .try_for_each(|key| match key {
                        Exec::Filter(k) => {
                            if filter.access_set(|_, m| m.contains_key(k.as_str())) {
                                Ok(())
                            } else {
                                Err(ConfigError::InvalidExecKey(key.as_ref().into(), k.clone())
                                    .into())
                            }
                        }
                    })
                    .map(|_| vec)
            })
            .log(Level::ERROR)?;

        Ok(Self { filter, join, exec })
    }

    pub fn get_filter(&self) -> &FilterSet {
        &self.filter
    }

    pub fn get_join(&self) -> &JoinSet {
        &self.join
    }

    pub fn get_exec(&self) -> impl Iterator<Item = OpKind<'_>> {
        self.exec.iter().map(|i| i.into())
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

impl Into<Subject> for Vec<Exec> {
    fn into(self) -> Subject {
        Subject::Exec
    }
}

fn lift_result<T>(cur: Option<Result<T>>, prev: &mut Option<Result<T>>) -> Result<()>
where
    T: Into<Subject>,
{
    use std::mem::swap;
    if let Some(mut cur) = cur {
        match prev {
            None => *prev = Some(cur),
            Some(prev) => match (cur.is_ok(), prev.is_ok()) {
                (true, false) | (false, false) => swap(&mut cur, prev),
                (true, true) => Err(ConfigError::Duplicate(cur.ok().take().unwrap().into()))?,
                (false, true) => (),
            },
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(from = "CfgInner")]
struct ConfigDeserialize {
    filter: Option<Result<FilterSet>>,
    join: Option<Result<JoinSet>>,
    exec: Option<Vec<Exec>>,
}

impl From<CfgInner> for ConfigDeserialize {
    fn from(inner: CfgInner) -> Self {
        use std::convert::TryInto;
        Self {
            filter: inner
                .filter
                .map(|i| i.try_into().map_err(|e| ConfigError::Other(e).into())),
            join: inner
                .join
                .map(|i| i.try_into().map_err(|e| ConfigError::Other(e).into())),
            exec: inner.exec,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CfgInner {
    #[serde(deserialize_with = "de_infallible", flatten)]
    filter: Option<FilterWrap>,
    #[serde(deserialize_with = "de_infallible", flatten)]
    join: Option<JoinWrap>,
    #[serde(deserialize_with = "de_infallible")]
    exec: Option<Vec<Exec>>,
}

fn de_infallible<'de, D, T>(de: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Deserialize::deserialize(de).map(Some).unwrap_or(None))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Exec {
    Filter(String),
}

impl Into<Subject> for &Exec {
    fn into(self) -> Subject {
        match self {
            Exec::Filter(_) => Subject::Filter,
        }
    }
}

impl AsRef<Exec> for Exec {
    fn as_ref(&self) -> &Exec {
        &self
    }
}

impl<'cli> From<&'cli Exec> for OpKind<'cli> {
    fn from(exec: &'cli Exec) -> Self {
        match exec {
            Exec::Filter(s) => OpKind::Filter(s.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OpKind<'cli> {
    Filter(&'cli str),
}
