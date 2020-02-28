use {
    crate::prelude::{CrateResult as Result, *},
    serde_interface::Record,
    tokio::net::TcpListener,
};

pub async fn listener(addr: &str) -> Result<()> {
    let mut listener = TcpListener::bind(addr)
        .await
        .map_err(|e| e.into())
        .log(Level::ERROR)?;

    // loop {
    //     let (mut socket, _addr) = listener.accept().await?;

    //     tokio::spawn(async move {
    //         let iter = Deserializer::from_reader(socket);
    //     });
    // }

    Ok(())
}
