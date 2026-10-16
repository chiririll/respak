//! Typed target options (PROJECT.md §5.5).

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value as Json};

use crate::error::{Error, ErrorKind, Result};
use crate::mediator::Target;

/// A target's options parsed into a plugin's own struct.
///
/// Fields the request leaves out come from `T::default()` (the request's fields are laid over
/// the serialized defaults, so this does not depend on `#[serde(default)]`). A field that does
/// not parse, or a request whose options are not an object, is `InvalidOptions`; a plugin that
/// wants unknown fields rejected puts `#[serde(deny_unknown_fields)]` on its struct.
///
/// [`TypedTarget::normalized`] is the options with every default filled in and the fields
/// sorted, which is what a value key needs (`ValueKey::converted`), so `{}` and
/// `{"mips":"first"}` give the same key.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedTarget<T> {
    options: T,
    normalized: Json,
}

impl<T: DeserializeOwned + Serialize + Default> TypedTarget<T> {
    /// Parses `target.options`.
    pub fn parse(target: &Target) -> Result<Self> {
        let invalid = |message: String| Error::new(ErrorKind::InvalidOptions { message });
        let given = match &target.options {
            Json::Null => Map::new(),
            Json::Object(m) => m.clone(),
            other => return Err(invalid(format!("options must be an object, not {other}"))),
        };
        let mut merged =
            match serde_json::to_value(T::default()).map_err(|e| invalid(e.to_string()))? {
                Json::Object(m) => m,
                _ => Map::new(),
            };
        merged.extend(given);
        let options: T =
            serde_json::from_value(Json::Object(merged)).map_err(|e| invalid(e.to_string()))?;
        let normalized = serde_json::to_value(&options).map_err(|e| invalid(e.to_string()))?;
        Ok(Self {
            options,
            normalized,
        })
    }
}

impl<T> TypedTarget<T> {
    /// The parsed options.
    pub fn options(&self) -> &T {
        &self.options
    }

    /// The options as JSON with defaults filled in, for value keys.
    pub fn normalized(&self) -> &Json {
        &self.normalized
    }
}
