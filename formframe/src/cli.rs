#![allow(deprecated)]
use {
    crate::{
        error::{CfgErrSubject as Subject, ConfigError},
        load::filters::{FilterSet, FilterWrap, JoinSet, JoinWrap},
        prelude::{CrateResult as Result, *},
    },
    clap::{crate_authors, crate_version, App, AppSettings, Arg, SubCommand},
    serde::{Deserialize, Deserializer},
    serde_yaml::from_reader as read_yaml,
    std::{
        convert::{TryFrom, TryInto},
        fs::File,
        net::ToSocketAddrs,
        path::Path,
    },
};

pub fn generate_cli<'a, 'b>() -> App<'a, 'b> {
    App::new("skipframe")
        .about("This program transforms input streams")
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("config-file")
                .short("f")
                .long("file")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .value_name("PATH")
                .required(true)
                .validator(|s| Some(s.as_str()).filter(|s| Path::new(s).exists()).map(|_| ())
                    .ok_or_else(|| format!("'{}' does not exist or is an invalid path", s)))
                .help("Read a config file, can be called multiple times (--help for more information)")
                .long_help("Read a config file, can be called multiple times. This program requires 'filter', 'map' \
                            'transform' and 'execute' objects, along with an optional 'config' object. These do not \
                            need to be stored in the same file, but each file needs to be valid .yaml and each object \
                            should be passed only once.")
        )
        .subcommand(
        SubCommand::with_name("tcp")
            .about("Listen on tcp")
            .arg(
                Arg::with_name("tcp-addr")
                .takes_value(false)
                .value_name("HOST:PORT")
                .default_value("localhost:8080")
                .validator(|val| {
                    val.as_str().to_socket_addrs()
                        .map(|_| ())
                        .map_err(|e| format!("Unable to resolve '{}': {}", val, e))
                    }
                )
                .help("Hostname/IP & Port to listen on")
            )
        )
}

pub struct ProgramArgs {
    bind: String,
    filter: FilterSet,
    join: JoinSet,
    exec: ExecList,
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

        let bind: String = match store.subcommand() {
            ("tcp", Some(store)) => store.value_of("tcp-addr").unwrap().to_string(),
            _ => unreachable!("No subcommand selected... this is a bug"),
        };

        let (filter, join, exec) = store
            .values_of("config-file")
            .map(|iter| instantiate_sets(iter))
            .unwrap()?;

        Ok(Self {
            bind,
            filter,
            join,
            exec,
        })
    }

    pub fn get_filter(&self) -> &FilterSet {
        &self.filter
    }

    pub fn get_join(&self) -> &JoinSet {
        &self.join
    }

    pub fn get_exec_list(&self) -> &ExecList {
        &self.exec
    }

    pub fn bind_addr(&self) -> &str {
        &self.bind
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

impl Into<Subject> for ExecList {
    fn into(self) -> Subject {
        Subject::Exec
    }
}

type Sets = (FilterSet, JoinSet, ExecList);

fn instantiate_sets<I, S>(mut iter: I) -> Result<Sets>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    let mut filter: Option<Result<FilterSet>> = None;
    let mut join: Option<Result<JoinSet>> = None;
    let mut exec: Option<Result<ExecList>> = None;

    // We allow the user to specify multiple files with a requirement that somewhere in
    // these files are all the required config options. Which means that if we can't open a file,
    // or if the file is invalid yaml we shouldn't give up because other files may contain the
    // information we need
    iter.try_for_each(|path| {
        debug_span!("cfg.load", file = path.as_ref());
        let file = File::open(path.as_ref());
        file.map_err(|e| e.into())
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
    })?;

    // Check to make sure we have all the required information
    let filter = filter
        .transpose()
        .and_then(|o| o.ok_or_else(|| ConfigError::Missing(Subject::Filter).into()))
        .log(Level::ERROR)?;
    let join = join
        .transpose()
        .and_then(|o| o.ok_or_else(|| ConfigError::Missing(Subject::Join).into()))
        .log(Level::ERROR)?;
    let exec = exec
        .transpose()
        .and_then(|o| o.ok_or_else(|| ConfigError::Missing(Subject::Join).into()))
        .and_then(|vec| {
            vec.inner
                .iter()
                .try_for_each(|key| match key {
                    DataOp::Filter(k) => {
                        if filter.access_set(|_, m| m.contains_key(k.as_str())) {
                            Ok(())
                        } else {
                            Err(ConfigError::InvalidExecKey(key.as_ref().into(), k.clone()).into())
                        }
                    }
                    DataOp::Load(_) | DataOp::Join => Ok(()),
                })
                .map(|_| vec)
        })
        .log(Level::ERROR)?;

    Ok((filter, join, exec))
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
                (true, true) => {
                    return Err(ConfigError::Duplicate(cur.ok().take().unwrap().into()).into())
                }
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
    exec: Option<ExecList>,
}

impl From<CfgInner> for ConfigDeserialize {
    fn from(inner: CfgInner) -> Self {
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
    exec: Option<ExecList>,
}

fn de_infallible<'de, D, T>(de: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Deserialize::deserialize(de).map(Some).unwrap_or(None))
}

#[derive(Debug, Deserialize)]
#[serde(from = "Vec<DataOp>")]
pub struct ExecList {
    inner: Vec<DataOp>,
    ops_r: Option<(usize, usize)>,
    load_r: Option<(usize, usize)>,
}

impl ExecList {
    fn new(backing: Vec<DataOp>) -> Self {
        let mut inner = backing;
        inner.sort();
        inner.dedup_by(|a, b| {
            if *a == *b && *b == DataOp::Join {
                true
            } else {
                false
            }
        });

        let ops_r = inner
            .iter()
            .enumerate()
            .take_while(|(_, op)| op.is_join() || op.is_filter())
            .fold(None, |state, (idx, _)| {
                state
                    .map(|(start, end)| (start, end + 1))
                    .or(Some((idx, idx + 1)))
            });

        let load_r = inner
            .iter()
            .enumerate()
            .skip_while(|(_, op)| !op.is_load())
            .take_while(|(_, op)| op.is_load())
            .fold(None, |state, (idx, _)| {
                state
                    .map(|(start, end)| (start, end + 1))
                    .or(Some((idx, idx + 1)))
            });

        Self {
            inner,
            ops_r,
            load_r,
        }
    }

    pub fn get_ops(&self) -> Option<impl Iterator<Item = OpKind<'_>>> {
        self.ops_r.as_ref().and_then(|&(s, e)| {
            self.inner.get(s..e).map(|sub| {
                sub.iter()
                    .map(|i| i.try_into())
                    .filter_map(std::result::Result::ok)
            })
        })
    }

    pub fn get_loads(&self) -> Option<impl Iterator<Item = Load<'_>>> {
        self.load_r.as_ref().and_then(|&(s, e)| {
            self.inner.get(s..e).map(|sub| {
                sub.iter()
                    .map(|i| i.try_into())
                    .filter_map(std::result::Result::ok)
            })
        })
    }
}

impl From<Vec<DataOp>> for ExecList {
    fn from(backing: Vec<DataOp>) -> Self {
        ExecList::new(backing)
    }
}

// Note that the order of variants in this enum are not arbitrary!
// Due to the Ord derive the variants must appear in this order for
// program correctness: Join, Filter, ..., Load
#[derive(Debug, Deserialize, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
enum DataOp {
    Join,
    Filter(String),
    Load(String),
}

impl DataOp {
    fn is_join(&self) -> bool {
        *self == Self::Join
    }

    fn is_filter(&self) -> bool {
        *self == Self::Filter(Default::default())
    }

    fn is_load(&self) -> bool {
        *self == Self::Load(Default::default())
    }
}

impl PartialEq for DataOp {
    fn eq(&self, other: &Self) -> bool {
        match (&self, other) {
            (Self::Join, Self::Join) => true,
            (Self::Filter(_), Self::Filter(_)) => true,
            (Self::Load(_), Self::Load(_)) => true,
            _ => false,
        }
    }
}

impl Into<Subject> for &DataOp {
    fn into(self) -> Subject {
        match self {
            DataOp::Join => Subject::Join,
            DataOp::Filter(_) => Subject::Filter,
            DataOp::Load(_) => Subject::Load,
        }
    }
}

impl AsRef<DataOp> for DataOp {
    fn as_ref(&self) -> &DataOp {
        &self
    }
}

impl<'cli> TryFrom<&'cli DataOp> for OpKind<'cli> {
    type Error = ();

    fn try_from(exec: &'cli DataOp) -> std::result::Result<Self, Self::Error> {
        match exec {
            DataOp::Join => Ok(OpKind::Join),
            DataOp::Filter(s) => Ok(OpKind::Filter(s.as_str())),
            _ => Err(()),
        }
    }
}

impl<'cli> TryFrom<&'cli DataOp> for Load<'cli> {
    type Error = ();

    fn try_from(exec: &'cli DataOp) -> std::result::Result<Self, Self::Error> {
        match exec {
            DataOp::Load(s) => Ok(Load(s.as_str())),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OpKind<'cli> {
    Filter(&'cli str),
    Join,
}

pub struct Load<'cli>(pub &'cli str);
