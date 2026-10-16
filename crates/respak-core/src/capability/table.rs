//! The [`Table`] capability.

/// The type of a [`Column`]; every cell of the column holds a value of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnType {
    /// [`Cell::Bool`].
    Bool,
    /// [`Cell::Int`].
    Int,
    /// [`Cell::Float`].
    Float,
    /// [`Cell::Vector`].
    Vector,
    /// [`Cell::String`].
    String,
    /// [`Cell::String`]: the path of an asset.
    Asset,
    /// [`Cell::String`]: the path of an asset that is not precached.
    AssetNoPrecache,
}

impl ColumnType {
    /// The name: `bool`, `int`, `float`, `vector`, `string`, `asset` or `asset_noprecache`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Vector => "vector",
            Self::String => "string",
            Self::Asset => "asset",
            Self::AssetNoPrecache => "asset_noprecache",
        }
    }
}

/// A named, typed column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// The column's name.
    pub name: String,
    /// The type of its cells.
    pub ty: ColumnType,
}

/// One value of a row.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// A boolean.
    Bool(bool),
    /// A signed 32-bit integer.
    Int(i32),
    /// A 32-bit float.
    Float(f32),
    /// Three 32-bit floats.
    Vector([f32; 3]),
    /// Text (also asset paths).
    String(String),
}

/// Rows and columns of typed cells, in memory: a row has one cell per column, in column order.
pub trait Table: Send + Sync {
    /// The columns.
    fn columns(&self) -> &[Column];
    /// The rows.
    fn rows(&self) -> &[Vec<Cell>];
}
