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
    std::fmt,
};

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub enum Record {
    #[serde(rename = "ss")]
    StreamStart,
    #[serde(rename = "se")]
    StreamEnd,
    #[serde(rename = "h")]
    Header(Header),
    #[serde(rename = "d")]
    Data(Data),
    #[serde(rename = "l")]
    Log(Log),
    #[serde(rename = "e")]
    Error(Error),
}

impl Record {
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

#[derive(Debug)]
pub struct Data {
    pub required: Common,
    pub time: i64,
    pub id: String,
    pub pid: u32,
    pub cxt: DataContext,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct Header {
    pub required: Common,
    pub time: i64,
    pub id: String,
    pub pid: u32,
    pub cxt: DataContext,
}

#[derive(Debug)]
pub struct Error {
    pub required: Common,
    pub error: CrateError,
}

#[derive(Debug)]
pub struct Log {
    pub required: Common,
    pub log: String,
}

/// Contains any records that are common to every record kind
#[derive(Debug)]
pub struct Common {
    pub version: u32,
}

impl Common {
    pub fn new(version: u32) -> Self {
        Self { version }
    }
}

impl Serialize for Data {
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
        map.serialize_entry(&TagMarker::Data, &self.data)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Data {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DataVisitor;

        impl<'de> Visitor<'de> for DataVisitor {
            type Value = Data;

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
                    id: id.ok_or_else(|| de::Error::missing_field("id"))?,
                    pid: pid.ok_or_else(|| de::Error::missing_field("pid"))?,
                    cxt: cxt.ok_or_else(|| de::Error::missing_field("cxt"))?,
                    data: data.ok_or_else(|| de::Error::missing_field("data"))?,
                })
            }
        }

        const FIELDS: &[&str] = &["required", "time", "id", "pid", "data"];
        deserializer.deserialize_struct("Data", FIELDS, DataVisitor)
    }
}

impl Serialize for Header {
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

impl<'de> Deserialize<'de> for Header {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HeaderVisitor;

        impl<'de> Visitor<'de> for HeaderVisitor {
            type Value = Header;

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
                    id: id.ok_or_else(|| de::Error::missing_field("id"))?,
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
