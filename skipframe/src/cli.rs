#![allow(deprecated)]
use {
    clap::{crate_authors, crate_version, App, Arg, SubCommand},
    std::{
        net::{IpAddr, SocketAddr},
        path::{Path, PathBuf},
        str::FromStr,
    },
};

#[cfg(unix)]
pub fn generate_cli<'a, 'b>() -> App<'a, 'b> {
    __generate_cli().subcommand(
        SubCommand::with_name("socket")
            .about("Use a unix socket for output")
            .arg(
                Arg::with_name("socket_connect")
                    .takes_value(false)
                    .value_name("PATH")
                    .required(true)
                    .validator(|val| match PathBuf::from(&val).exists() {
                        true => Ok(()),
                        false => Err(format!("'{}' does not exist or is an invalid path", &val)),
                    })
                    .help("Connect to socket at PATH"),
            ),
    )
}

#[cfg(not(unix))]
pub fn generate_cli<'a, 'b>() -> App<'a, 'b> {
    __generate_cli()
}

/// Generates base CLI without architecture specific options
fn __generate_cli<'a, 'b>() -> App<'a, 'b> {
    App::new("skipframe")
        .about("Reads and executes files from a given directory")
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .arg(
            Arg::with_name("exec_root")
                .takes_value(false)
                .value_name("PATH")
                .default_value(".")
                .help("Point at directory root of files to execute"),
        )
        .subcommand(
            SubCommand::with_name("tcp")
                .about("Use a tcp socket for output")
                .arg(
                    Arg::with_name("tcp_addr")
                        .takes_value(false)
                        .value_names(&["IPV4", "IPV6"])
                        .default_value("127.0.0.1")
                        .validator(|val| {
                            IpAddr::from_str(val.as_str())
                                .map(|_| ())
                                .map_err(|e| format!("'{}' is an {}", val, e))
                        })
                        .help("Connect to the given IP"),
                )
                .arg(
                    Arg::with_name("tcp_port")
                        .takes_value(false)
                        .value_name("PORT")
                        .default_value("50000")
                        .validator(|val| {
                            val.parse::<u16>()
                                .map(|_| ())
                                .map_err(|_| format!("'{}' is not a valid port", &val))
                        })
                        .help("On the given port"),
                ),
        )
}

pub(crate) struct ProgramArgs {
    exec_root: PathBuf,
    con_type: ConOpts,
}

impl ProgramArgs {
    /// Retains relevant user defined config settings gathered from the CLI
    pub(crate) fn init(cli: App<'_, '_>) -> Self {
        let store = cli.get_matches();

        let exec_root = PathBuf::from(store.value_of("exec_root").unwrap().to_string());

        let con_type;
        match store.subcommand() {
            ("socket", Some(sub)) => {
                con_type =
                    ConOpts::UnixSocket(PathBuf::from(sub.value_of("socket_connect").unwrap()))
            }
            ("tcp", Some(sub)) => {
                let ip = IpAddr::from_str(sub.value_of("tcp_addr").unwrap()).unwrap();
                let port = sub
                    .value_of("tcp_port")
                    .map(|s| s.parse::<u16>().unwrap())
                    .unwrap();
                con_type = ConOpts::Tcp(SocketAddr::new(ip, port))
            }
            _ => con_type = ConOpts::default(),
        }

        Self {
            exec_root,
            con_type,
        }
    }

    /// Return user's specified path root
    pub(crate) fn exec_root(&self) -> &Path {
        &self.exec_root
    }

    /// If the user selected a TCP stream, returns the address.
    /// Guaranteed to be Some if con_socket() and con_stdout() are None
    pub(crate) fn con_tcp(&self) -> Option<SocketAddr> {
        match self.con_type {
            ConOpts::Tcp(addr) => Some(addr),
            _ => None,
        }
    }

    /// If the user selected a unix stream, returns the path.
    /// Guaranteed to be Some if con_tcp() and con_stdout() are None.
    /// NOTE: always returns None on unsupported architecture
    pub(crate) fn con_socket(&self) -> Option<&Path> {
        if cfg!(target_family = "unix") {
            match self.con_type {
                ConOpts::UnixSocket(ref path) => Some(path.as_ref()),
                _ => None,
            }
        } else {
            None
        }
    }

    /// If the user did not select an output stream, returns Some.
    /// Guaranteed to be Some if con_tcp() and con_socket() are None
    pub(crate) fn con_stdout(&self) -> Option<()> {
        match self.con_type {
            ConOpts::Stdout => Some(()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
#[cfg(unix)]
/// Possible output streams
enum ConOpts {
    Stdout,
    Tcp(SocketAddr),
    UnixSocket(PathBuf),
}

#[derive(Debug, Clone)]
#[cfg(not(unix))]
/// Possible output streams
enum ConOpts {
    Stdout,
    Tcp(SocketAddr),
}

impl Default for ConOpts {
    fn default() -> Self {
        Self::Stdout
    }
}
