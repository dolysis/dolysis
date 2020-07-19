use {
    lib_transport::{
        Common as RecordCommon, Data as RecordData, DataContext, Error as RecordError,
        Header as RecordHeader, InterfaceError, Log as RecordLog, Record,
    },
    serde::{Deserialize, Serialize},
};
#[derive(Debug, Serialize, Deserialize)]
pub(super) enum LocalRecord {
    StreamStart,
    StreamEnd,
    Header(Header),
    Data(Data),
    Log(Log),
    Error(Error),
}

impl From<Record<'_, '_>> for LocalRecord {
    fn from(record: Record) -> Self {
        match record {
            Record::StreamStart => LocalRecord::StreamStart,
            Record::StreamEnd => LocalRecord::StreamEnd,
            Record::Header(r) => LocalRecord::Header(r.into()),
            Record::Data(r) => LocalRecord::Data(r.into()),
            Record::Log(r) => LocalRecord::Log(r.into()),
            Record::Error(r) => LocalRecord::Error(r.into()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Data {
    required: Common,
    time: i64,
    id: String,
    pid: u32,
    cxt: Context,
    data: String,
}

impl From<RecordData<'_, '_>> for Data {
    fn from(r: RecordData) -> Self {
        Self {
            required: r.required.into(),
            time: r.time,
            id: r.id.into(),
            pid: r.pid,
            cxt: r.cxt.into(),
            data: r.data.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Header {
    required: Common,
    time: i64,
    id: String,
    pid: u32,
    cxt: Context,
}

impl From<RecordHeader<'_>> for Header {
    fn from(r: RecordHeader) -> Self {
        Self {
            required: r.required.into(),
            time: r.time,
            id: r.id.into(),
            pid: r.pid,
            cxt: r.cxt.into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Error {
    required: Common,
    error: InterfaceError,
}

impl From<RecordError> for Error {
    fn from(r: RecordError) -> Self {
        Self {
            required: r.required.into(),
            error: r.error,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Log {
    required: Common,
    log: String,
}

impl From<RecordLog> for Log {
    fn from(r: RecordLog) -> Self {
        Self {
            required: r.required.into(),
            log: r.log,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Common {
    version: u32,
}

impl From<RecordCommon> for Common {
    fn from(r: RecordCommon) -> Self {
        Self { version: r.version }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) enum Context {
    Start,
    End,
    Stdout,
    Stderr,
}

impl From<DataContext> for Context {
    fn from(cxt: DataContext) -> Self {
        match cxt {
            DataContext::Start => Self::Start,
            DataContext::End => Self::End,
            DataContext::Stderr => Self::Stderr,
            DataContext::Stdout => Self::Stdout,
        }
    }
}
