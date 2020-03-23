use {
    crate::{
        error::CrateError,
        markers::{DataContext, KindMarker, TagMarker},
        traits::Marker,
    },
    serde::{
        de::{self, Deserializer, IgnoredAny, MapAccess, Visitor},
        ser::{SerializeMap, Serializer},
        {Deserialize, Serialize},
    },
    std::{borrow::Cow, fmt},
};

/// The in-memory representation of a Record. This is the mechanism by which the
/// binaries transmit information across the wire. This struct has an intentionally
/// minimalistic API. Any manipulation should be done via some local representation,
/// making use of From/Into (or some similar interface) for moving into and out of
/// said representation.
///
/// As an aside, this structure's Serde impl is optimized for size and _highly_ unlikely
/// to de/serialize into a valid Record if the data is not serialized *and* deserialized as this struct.
/// Do not attempt to de/serialize into some intermediary struct. It will end badly.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Record<'i, 'd> {
    #[serde(rename = "ss")]
    StreamStart,
    #[serde(rename = "se")]
    StreamEnd,
    #[serde(rename = "h")]
    Header(Header<'i>),
    #[serde(rename = "d")]
    Data(Data<'i, 'd>),
    #[serde(rename = "l")]
    Log(Log),
    #[serde(rename = "e")]
    Error(Error),
}

impl<'i, 'd> Record<'i, 'd> {
    /// Convenience function for generating Record errors
    pub fn new_error<E>(version: u32, err: E) -> Self
    where
        E: Into<CrateError>,
    {
        Self::Error(Error {
            required: Common::new(version),
            error: err.into(),
        })
    }
}

/// A hacky trapdoor for creating a Record. It is the users responsibility
/// to ensure that the 'Record' is a valid Record kind (i.e: a `Header` or `Data`)
// TODO: This really should be removed, it is a workaround for serializing non-owned data,
// i.e a &[u8] instead of a Vec<u8>. Realistically, to solve this problem we would need to
// create some sort of intermediary structure that both an Owned and Borrowed Record
// would de/serialize as.
#[derive(Debug, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum RecordKind<R> {
    #[serde(rename = "ss")]
    StreamStart,
    #[serde(rename = "se")]
    StreamEnd,
    #[serde(rename = "h")]
    Header(R),
    #[serde(rename = "d")]
    Data(R),
    #[serde(rename = "l")]
    Log(R),
    #[serde(rename = "e")]
    Error(R),
}

impl<R> RecordKind<R> {
    /// Create a new RecordKind. It is the user's responsibility to ensure that the given `R` is a valid
    /// record.
    pub fn new<M>(mkr: M, record: R) -> Self
    where
        M: Marker<Marker = KindMarker>,
    {
        match mkr.as_marker() {
            KindMarker::StreamStart => Self::StreamStart,
            KindMarker::StreamEnd => Self::StreamEnd,
            KindMarker::Header => Self::Header(record),
            KindMarker::Data => Self::Data(record),
            KindMarker::Error => Self::Error(record),
            KindMarker::Log => Self::Log(record),
        }
    }
}

/// Contains a byte slice and related context. This slice contains some unit of data that is conceptually
/// whole or 'one' for its intended destination. It should be preceded by _one_ header record, `Context::Start`
/// and any number of other `Data` records. It should be followed by any number of `Data` records and a single Header `Context::End`
#[derive(Debug)]
pub struct Data<'i, 'd> {
    pub required: Common,
    pub time: i64,
    pub id: Cow<'i, str>,
    pub pid: u32,
    pub cxt: DataContext,
    pub data: Cow<'d, str>,
}

/// A header / tail record for gracefully terminating a stream of Data records. Conceptually, it is responsible for starting
/// and terminating a stream of `Data` records
#[derive(Debug)]
pub struct Header<'i> {
    pub required: Common,
    pub time: i64,
    pub id: Cow<'i, str>,
    pub pid: u32,
    pub cxt: DataContext,
}

/// Contains any error messages that were caused by an unexpected / non-graceful termination of a project binary
#[derive(Debug)]
pub struct Error {
    pub required: Common,
    pub error: CrateError,
}

/// Contains any log messages that were produced by a project binary up the data stream.
#[derive(Debug)]
pub struct Log {
    pub required: Common,
    pub log: String,
}

/// Contains any fields that are common to every record kind
#[derive(Debug)]
pub struct Common {
    pub version: u32,
}

impl Common {
    pub fn new(version: u32) -> Self {
        Self { version }
    }
}

impl<'i, 'd> Serialize for Data<'i, 'd> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry(&TagMarker::Version, &self.required.version)?;
        map.serialize_entry(&TagMarker::Time, &self.time)?;
        map.serialize_entry(&TagMarker::Id, &self.id)?;
        map.serialize_entry(&TagMarker::Pid, &self.pid)?;
        map.serialize_entry(&TagMarker::DataContext, &self.cxt)?;
        map.serialize_entry(&TagMarker::Data, self.data.as_ref())?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Data<'_, '_> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DataVisitor;

        impl<'de> Visitor<'de> for DataVisitor {
            type Value = Data<'static, 'static>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("Expecting a valid 'Data' record")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                macro_rules! checked_set {
                    ($var:ident) => {{
                        if $var.is_some() {
                            return Err(de::Error::duplicate_field("$var"));
                        }
                        $var = Some(map.next_value()?);
                    }};
                }
                let mut version = None;
                let mut time = None;
                let mut id = None;
                let mut pid = None;
                let mut cxt = None;
                let mut data = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        TagMarker::Version => checked_set!(version),
                        TagMarker::Time => checked_set!(time),
                        TagMarker::Id => checked_set!(id),
                        TagMarker::Pid => checked_set!(pid),
                        TagMarker::DataContext => checked_set!(cxt),
                        TagMarker::Data => checked_set!(data),
                        _ => {
                            let _ignored: IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(Self::Value {
                    required: Common {
                        version: version.ok_or_else(|| de::Error::missing_field("version"))?,
                    },
                    time: time.ok_or_else(|| de::Error::missing_field("time"))?,
                    id: id
                        .map(|cow: String| cow.into())
                        .ok_or_else(|| de::Error::missing_field("id"))?,
                    pid: pid.ok_or_else(|| de::Error::missing_field("pid"))?,
                    cxt: cxt.ok_or_else(|| de::Error::missing_field("cxt"))?,
                    data: data
                        .map(|cow: String| cow.into())
                        .ok_or_else(|| de::Error::missing_field("data"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["required", "time", "id", "pid", "data"];
        deserializer.deserialize_struct("Data", FIELDS, DataVisitor)
    }
}

impl<'i> Serialize for Header<'i> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry(&TagMarker::Version, &self.required.version)?;
        map.serialize_entry(&TagMarker::Time, &self.time)?;
        map.serialize_entry(&TagMarker::Id, &self.id)?;
        map.serialize_entry(&TagMarker::DataContext, &self.cxt)?;
        map.serialize_entry(&TagMarker::Pid, &self.pid)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Header<'_> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HeaderVisitor;

        impl<'de> Visitor<'de> for HeaderVisitor {
            type Value = Header<'static>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("Expecting a valid 'Header' record")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                macro_rules! checked_set {
                    ($var:ident) => {{
                        if $var.is_some() {
                            return Err(de::Error::duplicate_field("$var"));
                        }
                        $var = Some(map.next_value()?);
                    }};
                }
                let mut version = None;
                let mut time = None;
                let mut id = None;
                let mut pid = None;
                let mut cxt = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        TagMarker::Version => checked_set!(version),
                        TagMarker::Time => checked_set!(time),
                        TagMarker::Id => checked_set!(id),
                        TagMarker::DataContext => checked_set!(cxt),
                        TagMarker::Pid => checked_set!(pid),
                        _ => {
                            let _ignored: IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(Self::Value {
                    required: Common {
                        version: version.ok_or_else(|| de::Error::missing_field("version"))?,
                    },
                    time: time.ok_or_else(|| de::Error::missing_field("time"))?,
                    id: id
                        .map(|cow: String| cow.into())
                        .ok_or_else(|| de::Error::missing_field("id"))?,
                    pid: pid.ok_or_else(|| de::Error::missing_field("pid"))?,
                    cxt: cxt.ok_or_else(|| de::Error::missing_field("cxt"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["required", "time", "id", "pid"];
        deserializer.deserialize_struct("Header", FIELDS, HeaderVisitor)
    }
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry(&TagMarker::Version, &self.required.version)?;
        map.serialize_entry(&TagMarker::Error, &self.error)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Error {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ErrorVisitor;

        impl<'de> Visitor<'de> for ErrorVisitor {
            type Value = Error;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("Expecting a valid 'Error' record")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                macro_rules! checked_set {
                    ($var:ident) => {{
                        if $var.is_some() {
                            return Err(de::Error::duplicate_field("$var"));
                        }
                        $var = Some(map.next_value()?);
                    }};
                }
                let mut version = None;
                let mut error = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        TagMarker::Version => checked_set!(version),
                        TagMarker::Utf8Data => checked_set!(error),
                        _ => {
                            let _ignored: IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(Self::Value {
                    required: Common {
                        version: version.ok_or_else(|| de::Error::missing_field("version"))?,
                    },
                    error: error.ok_or_else(|| de::Error::missing_field("error"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["required", "error"];
        deserializer.deserialize_struct("Error", FIELDS, ErrorVisitor)
    }
}

impl Serialize for Log {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry(&TagMarker::Version, &self.required.version)?;
        map.serialize_entry(&TagMarker::Utf8Data, &self.log)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Log {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LogVisitor;

        impl<'de> Visitor<'de> for LogVisitor {
            type Value = Log;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("Expecting a valid 'Log' record")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: MapAccess<'de>,
            {
                macro_rules! checked_set {
                    ($var:ident) => {{
                        if $var.is_some() {
                            return Err(de::Error::duplicate_field("$var"));
                        }
                        $var = Some(map.next_value()?);
                    }};
                }

                let mut version = None;
                let mut log = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        TagMarker::Version => checked_set!(version),
                        TagMarker::Utf8Data => checked_set!(log),
                        _ => {
                            let _ignored: IgnoredAny = map.next_value()?;
                        }
                    }
                }
                Ok(Self::Value {
                    required: Common {
                        version: version.ok_or_else(|| de::Error::missing_field("version"))?,
                    },
                    log: log.ok_or_else(|| de::Error::missing_field("log"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["required", "log"];
        deserializer.deserialize_struct("Log", FIELDS, LogVisitor)
    }
}
