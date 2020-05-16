#![allow(deprecated)]
use {
    clap::{crate_authors, crate_version, App, AppSettings, Arg, SubCommand},
    std::path::{Path, PathBuf},
};

#[cfg(unix)]
pub fn generate_cli<'a, 'b>() -> App<'a, 'b> {
    __generate_cli().subcommand(
        SubCommand::with_name("socket")
            .about("Bind a unix socket for input")
            .arg(
                Arg::with_name("socket_connect")
                    .takes_value(false)
                    .value_name("PATH")
                    .required(true)
                    .validator(|val| match PathBuf::from(&val).exists() {
                        false => Ok(()),
                        true => Err(format!("'{}' already exists or is an invalid path", &val)),
                    })
                    .help("Bind socket listener to PATH"),
            ),
    )
}

#[cfg(not(unix))]
pub fn generate_cli<'a, 'b>() -> App<'a, 'b> {
    __generate_cli()
}

fn __generate_cli<'a, 'b>() -> App<'a, 'b> {
    App::new("skipframe")
        .about("Transcodes and prints cbor records as JSON")
        .author(crate_authors!("\n"))
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg(
            Arg::with_name("json_pretty")
                .takes_value(false)
                .long("pretty")
                .help("Pretty print json"),
        )
        .subcommand(
            SubCommand::with_name("tcp")
                .about("Bind a tcp socket for output")
                .arg(
                    Arg::with_name("tcp_addr")
                        .short("b")
                        .long("bind")
                        .value_names(&["HOST", "IP"])
                        .default_value("0.0.0.0")
                        .hide_default_value(true)
                        .help("Bind the given address, defaulting to all available"),
                )
                .arg(
                    Arg::with_name("tcp_port")
                        .short("p")
                        .long("port")
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
    con_type: ConOpts,
    pretty_print: bool,
}

impl ProgramArgs {
    pub(crate) fn init(cli: App<'_, '_>) -> Self {
        let store = cli.get_matches();

        let pretty_print = store.is_present("json_pretty");

        let con_type;
        match store.subcommand() {
            ("socket", Some(sub)) => {
                con_type =
                    ConOpts::UnixSocket(PathBuf::from(sub.value_of("socket_connect").unwrap()))
            }
            ("tcp", Some(sub)) => {
                let bind = sub.value_of("tcp_addr").unwrap().into();
                let port = sub
                    .value_of("tcp_port")
                    .map(|s| s.parse::<u16>().unwrap())
                    .unwrap();
                con_type = ConOpts::Tcp((bind, port))
            }
            _ => unreachable!(),
        }

        Self {
            con_type,
            pretty_print,
        }
    }

    pub(crate) fn pretty_print(&self) -> bool {
        self.pretty_print
    }

    pub(crate) fn con_tcp(&self) -> Option<(&str, u16)> {
        match self.con_type {
            ConOpts::Tcp((ref bind, port)) => Some((bind, port)),
            _ => None,
        }
    }

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
}

#[derive(Debug, Clone)]
#[cfg(unix)]
enum ConOpts {
    Tcp((String, u16)),
    UnixSocket(PathBuf),
}

#[derive(Debug, Clone)]
#[cfg(not(unix))]
enum ConOpts {
    Tcp(SocketAddr),
}
