use {
    crate::ARGS,
    serde_cbor::Deserializer,
    serde_interface::Record,
    serde_json::{to_writer, to_writer_pretty},
    std::{error, io, net::TcpListener},
};

pub fn process_single_stream() -> Result<(), Box<dyn error::Error>> {
    match (ARGS.con_socket(), ARGS.con_tcp()) {
        (Some(socket), _) => {
            if cfg!(target_family = "unix") {
                use std::os::unix::net::UnixListener;
                let listener = UnixListener::bind(socket).map_err(|e| Box::new(e))?;
                listener
                    .accept()
                    .map_err(|e| Box::new(e) as Box<dyn error::Error>)
                    .and_then(|(stream, addr)| {
                        println!("Got a connection from: {:?}", addr);
                        let mut iter = stream_transcode(stream);
                        let p = ARGS.pretty_print();
                        while let Some(record) = iter.next() {
                            match record? {
                                Record::StreamStart => (),
                                Record::StreamEnd => break,
                                record @ _ => print_json(p, io::stdout(), record)?,
                            }
                        }
                        Ok(())
                    })?;

                Ok(())
            } else {
                // Should not be possible to hit this path as con_socket() should always return None on
                // non-unix systems
                panic!("Attempted to use unix specific socket implementation on a non unix system")
            }
        }
        (_, Some(addr)) => {
            let listener = TcpListener::bind(addr).map_err(|e| Box::new(e))?;
            listener
                .accept()
                .map_err(|e| Box::new(e) as Box<dyn error::Error>)
                .and_then(|(stream, addr)| {
                    println!("Got a connection from: {:?}", addr);
                    let mut iter = stream_transcode(stream);
                    let p = ARGS.pretty_print();
                    while let Some(record) = iter.next() {
                        match record? {
                            Record::StreamStart => (),
                            Record::StreamEnd => break,
                            record @ _ => print_json(p, io::stdout(), record)?,
                        }
                    }
                    Ok(())
                })?;

            Ok(())
        }
        _ => unreachable!(),
    }
}

fn stream_transcode<R>(reader: R) -> impl Iterator<Item = Result<Record, Box<dyn error::Error>>>
where
    R: io::Read,
{
    Deserializer::from_reader(reader)
        .into_iter::<Record>()
        .map(|res| res.map_err(|e| e.into()))
}

fn print_json<W>(pretty: bool, writer: W, rcd: Record) -> Result<(), Box<dyn error::Error>>
where
    W: io::Write,
{
    match pretty {
        true => to_writer_pretty(writer, &rcd)?,
        false => to_writer(writer, &rcd)?,
    }
    Ok(())
}
