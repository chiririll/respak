use serde::{Deserialize, Serialize};

/// Which skeleton an `aseq` is bound to (the `rig` option): `"auto"` or a guid / path.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum RigSelect {
    /// The owner found by the reverse index: the first `arig`, else the first `mdl_`, that lists
    /// the sequence. Written `"auto"`.
    #[default]
    Auto,
    /// An `arig` / `mdl_` named by guid (`0x...`) or by path (hashed with `string_to_guid`).
    Explicit(String),
}

impl From<String> for RigSelect {
    fn from(s: String) -> Self {
        if s == "auto" {
            Self::Auto
        } else {
            Self::Explicit(s)
        }
    }
}

impl From<RigSelect> for String {
    fn from(s: RigSelect) -> Self {
        match s {
            RigSelect::Auto => "auto".into(),
            RigSelect::Explicit(name) => name,
        }
    }
}
