//! Format identifiers and versions.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, ErrorKind};

/// A format version.
///
/// `major` is the format's own version number. `minor` is the **layout variant**: the same
/// major version can hide several layouts (e.g. `matl@23.1` and `matl@23.2`), so a version
/// number alone is never proof of a layout. `minor` is 0 when the format has no variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    /// The format's version number.
    pub major: u32,
    /// The layout variant, 0 if none.
    pub minor: u32,
}

impl Version {
    /// A version with no layout variant.
    pub const fn new(major: u32) -> Self {
        Self { major, minor: 0 }
    }

    /// A version with a layout variant.
    pub const fn with_minor(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

/// Prints `10` or `23.2`; the variant is omitted when it is 0.
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == 0 {
            write!(f, "{}", self.major)
        } else {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::new(ErrorKind::InvalidRequest { message: format!("invalid version `{s}`") });
        let (major, minor) = s.split_once('.').unwrap_or((s, "0"));
        Ok(Self { major: major.parse().map_err(|_| bad())?, minor: minor.parse().map_err(|_| bad())? })
    }
}

/// A format name with an optional version, written `txtr`, `txtr@10` or `txtr@23.2`.
///
/// Names are owned by the crate that defines the format. Serialized as that string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormatId {
    name: Cow<'static, str>,
    version: Option<Version>,
}

impl FormatId {
    /// An unversioned id from a static name (usable in `const` context).
    pub const fn from_static(name: &'static str) -> Self {
        Self { name: Cow::Borrowed(name), version: None }
    }

    /// An unversioned id.
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), version: None }
    }

    /// The same format with a version.
    pub fn with_version(mut self, version: Version) -> Self {
        self.version = Some(version);
        self
    }

    /// The format name, e.g. `txtr`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The detected version, if known.
    pub fn version(&self) -> Option<Version> {
        self.version
    }

    /// The same format without its version.
    pub fn unversioned(&self) -> Self {
        Self { name: self.name.clone(), version: None }
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.version {
            Some(v) => write!(f, "{}@{}", self.name, v),
            None => f.write_str(&self.name),
        }
    }
}

impl FromStr for FormatId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let (name, version) = match s.split_once('@') {
            Some((n, v)) => (n, Some(v.parse()?)),
            None => (s, None),
        };
        if name.is_empty() {
            return Err(Error::new(ErrorKind::InvalidRequest { message: format!("invalid format id `{s}`") }));
        }
        Ok(Self { name: Cow::Owned(name.to_owned()), version })
    }
}

impl Serialize for FormatId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FormatId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}
