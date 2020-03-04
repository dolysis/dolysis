use {
    crate::prelude::*,
    std::{cmp::Ordering, convert::TryFrom},
    walkdir::DirEntry,
};

/// Helper function for WalkDir's .sort_by() function, encapsulates several fallible transformations
/// and provides a sensible Ordering to WalkDir
pub fn by_priority(a: &DirEntry, b: &DirEntry) -> Ordering {
    let (first, second) = (Priority::try_from(a), Priority::try_from(b));

    // Note that this block should agree with Priority's Ord impl,
    // assuming Ok => Priority::Number, Err => Priority::None
    // so that the sorting outcome always follows: [has_priority,no_priority,invalid_str]
    match (first, second) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => Ordering::Equal,
    }
}

/// Representation of a relevant dir entry's relative run priority
/// with the ordering: Higher Number > Lower Number > No Number
#[derive(Debug, Clone, Copy)]
pub enum Priority {
    Number(u64),
    None,
}

impl Priority {
    fn try_from_str(s: &str) -> Result<Self> {
        let numeric = str_take_while(s, |b| is_numeric(*b));

        if numeric.is_empty() {
            Ok(Self::None)
        } else {
            numeric
                .parse::<u64>()
                .map(Self::Number)
                .map_err(|e| e.into())
        }
    }
}

impl TryFrom<&DirEntry> for Priority {
    type Error = CrateError;

    fn try_from(entry: &DirEntry) -> std::result::Result<Self, Self::Error> {
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| Self::Error::from(entry.file_name().to_os_string()))?;

        Self::try_from_str(name)
    }
}

impl PartialEq for Priority {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::None, Self::Number(_)) => false,
            (Self::Number(_), Self::None) => false,
            (Self::None, Self::None) => true,
        }
    }
}

impl Eq for Priority {}

impl Ord for Priority {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            (Self::Number(_), Self::None) => Ordering::Less,
            (Self::None, Self::Number(_)) => Ordering::Greater,
            (Self::None, Self::None) => Ordering::Equal,
        }
    }
}

impl PartialOrd for Priority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Simple wrapper around Iterator's .take_while(), specialized for strs
fn str_take_while<P>(initial: &str, predicate: P) -> &str
where
    P: Fn(&u8) -> bool,
{
    let i = initial.bytes().take_while(|b| predicate(b)).count();
    // Unwrap over unchecked, ensure a panic rather than UB
    initial.get(0..i).unwrap()
}

/// Checks if a byte is an ASCII numeric byte
fn is_numeric(b: u8) -> bool {
    // Use ascii values to avoid the perf penalty of inclusive range
    (48..58).contains(&b)
}
