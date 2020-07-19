use {
    crate::{local::LocalRecord, prelude::*, ARGS},
    futures::prelude::*,
    lib_transport::RecordInterface,
    serde_json::{to_writer, to_writer_pretty},
    std::{io, path::Path},
    tokio::{net::TcpListener, prelude::AsyncRead},
    tracing_subscriber::{EnvFilter, FmtSubscriber},
};

pub async fn process_incoming() -> Result<(), io::Error> {
    match (ARGS.con_socket(), ARGS.con_tcp()) {
        (Some(socket), _) => {
            if cfg!(target_family = "unix") {
                use_unixsocket(socket)
                    .instrument(always_span!("server.unixsocket", socket = %socket.display()))
                    .await
            } else {
                // Should not be possible to hit this path as con_socket() should always return None on
                // non-unix systems
                panic!("Attempted to use unix specific socket implementation on a non unix system")
            }
        }
        (_, Some(addr)) => {
            use_tcp(addr)
                .instrument(always_span!("server.tcp", bind = %addr.0, port = addr.1))
                .await
        }
        _ => unreachable!(),
    }
}

#[cfg(unix)]
async fn use_unixsocket(socket: &Path) -> Result<(), io::Error> {
    use tokio::net::UnixListener;
    debug!("Attempting to bind {}...", socket.display());
    let mut listener = UnixListener::bind(socket)
        .map(|l| {
            info!("Bind successful, server is waiting on connections");
            l
        })
        .map_err(|e| {
            error!("Binding {} failed... bailing", socket.display());
            e
        })?;

    loop {
        listener
            .accept()
            .map_ok_or_else(
                |e| warn!("Failed to accept connection: {}", e),
                |(socket, client)| {
                    client
                        .as_pathname()
                        .map(|p| info!("Accepted connection from: {}", p.display()))
                        .unwrap_or_else(|| info!("Accepted connection from: unnamed"));

                    tokio::spawn(handle_connection(socket));
                },
            )
            .await
    }
}

async fn use_tcp(addr: (&str, u16)) -> Result<(), io::Error> {
    debug!("Attempting to bind {}:{}...", addr.0, addr.1);
    let mut listener = TcpListener::bind(addr)
        .inspect(|status| match status {
            Ok(_) => info!("Bind successful, server is waiting on connections"),
            Err(_) => error!("Binding {}:{} failed... bailing", addr.0, addr.1),
        })
        .await?;

    loop {
        listener
            .accept()
            .map_ok_or_else(
                |e| warn!("Failed to accept connection: {}", e),
                |(socket, client)| {
                    info!("Accepted connection from: {}", client);

                    tokio::spawn(handle_connection(socket));
                },
            )
            .await
    }
}

async fn handle_connection<T>(read: T)
where
    T: AsyncRead,
{
    let pretty = ARGS.pretty_print();
    RecordInterface::from_read(read)
        .for_each(|item| async {
            item.and_then(|record| print_json(pretty, io::stdout(), record.into()))
                .unwrap_or_else(|e| warn!("Item serialization failed: {}", e))
        })
        .instrument(always_span!("printer.json", pretty))
        .await
}

fn print_json<W>(pretty: bool, writer: W, rcd: LocalRecord) -> Result<(), io::Error>
where
    W: io::Write,
{
    match pretty {
        true => to_writer_pretty(writer, &rcd)?,
        false => to_writer(writer, &rcd)?,
    }
    Ok(())
}

/// Initialize the global logger. This function must be called before ARGS is initialized,
/// otherwise logs generated during CLI parsing will be silently ignored
pub fn init_logging() {
    let root_subscriber = FmtSubscriber::builder()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::default().add_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        }))
        .with_filter_reloading()
        .finish();
    tracing::subscriber::set_global_default(root_subscriber).expect("Failed to init logging");
    info!("<== Logs Start ==>")
}
