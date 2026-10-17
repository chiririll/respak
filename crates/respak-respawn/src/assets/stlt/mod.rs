//! `stlt`: a settings layout, read from `rpak.asset`. Apex Legends, asset version 0 (72-byte
//! header). It names and types the fields of the value buffer of a `stgs`.
//!
//! Ported from rsx `src/game/rtech/assets/settings_layout.{h,cpp}` (`SettingsLayoutHeader_v0_t`,
//! `SettingsField_t`, `SettingsLayoutAsset`). Read edge only: the layout is decoded into a tree
//! ([`Layout`]); the `stgs` reader uses it to decode its values.

mod json;

pub use json::StltJsonEncode;

mod read;
pub use read::StltRead;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use respak_core::capability::{Dependency, Metadata};
use respak_core::reader::Reader;
use respak_core::{
    Cx, Data, Detect, Detected, Error, ErrorKind, FormatId, Probe, ReadHandler, Result, Value,
    ValueKey, Version,
};

use crate::rmdl::reader;
use crate::rpak::{detect_kind, rpak_asset, RpakAsset, MEM};

/// Bytes of a layout header, also the stride of the sub layout array.
const HEADER_LEN: usize = 72;

/// Longest field name table taken (`nameOffset` is a `u16`).
const NAMES_WINDOW: u64 = 0x10000 + 256;

/// Nesting limit for sub layouts.
const DEPTH: usize = 8;

/// Largest hash table, field count or sub layout count taken as plausible.
const LIMIT: u32 = 1 << 20;

/// The type of a field (`eSettingsFieldType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// A bool.
    Bool,
    /// A 32-bit int.
    Int,
    /// A 32-bit float.
    Float,
    /// Two floats.
    Float2,
    /// Three floats.
    Float3,
    /// A string.
    String,
    /// An asset path.
    Asset,
    /// `asset_noprecache`: an asset path that is not preloaded.
    AssetNoPrecache,
    /// An inline array of sub layout elements.
    Array,
    /// `{i32 count, i32 offset}` in the buffer; the elements are at `offset` from its start.
    DynamicArray,
}

impl FieldType {
    fn from_u16(v: u16) -> Option<Self> {
        Some(match v {
            0 => Self::Bool,
            1 => Self::Int,
            2 => Self::Float,
            3 => Self::Float2,
            4 => Self::Float3,
            5 => Self::String,
            6 => Self::Asset,
            7 => Self::AssetNoPrecache,
            8 => Self::Array,
            9 => Self::DynamicArray,
            _ => return None,
        })
    }

    /// The name used in JSON.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Float2 => "float2",
            Self::Float3 => "float3",
            Self::String => "string",
            Self::Asset => "asset",
            Self::AssetNoPrecache => "asset_noprecache",
            Self::Array => "array",
            Self::DynamicArray => "array_dynamic",
        }
    }

    /// The alignment of a value (`None` for a static array: it takes its elements').
    fn alignment(self) -> Option<u32> {
        match self {
            Self::Bool => Some(1),
            Self::Int | Self::Float | Self::Float2 | Self::Float3 | Self::DynamicArray => Some(4),
            Self::String | Self::Asset | Self::AssetNoPrecache => Some(8),
            Self::Array => None,
        }
    }
}

/// One field of a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The name.
    pub name: String,
    /// The help text, empty if none.
    pub help: String,
    /// The type.
    pub ty: FieldType,
    /// Offset of the value from the start of the element.
    pub offset: u32,
    /// Index into [`Layout::subs`] for arrays.
    pub sub: u8,
}

/// A layout: the fields of one element and the layouts of its arrays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The name; sub layouts have none.
    pub name: Option<String>,
    /// Fields sorted by offset.
    pub fields: Vec<Field>,
    /// `arrayValueCount`: 1 for a plain layout, the element count of a static array, -1 for a
    /// dynamic array.
    pub element_count: i32,
    /// `totalBufferSize`: bytes of one element.
    pub total_size: u32,
    /// `extraDataSizeIndex`, kept as stored.
    pub extra_data_size_index: u32,
    /// The largest alignment of the fields.
    pub alignment: u32,
    /// The layouts of the array fields, indexed by [`Field::sub`].
    pub subs: Vec<Layout>,
}

impl Layout {
    /// Bytes between two elements of an array of this layout.
    pub fn stride(&self) -> u32 {
        let a = self.alignment.max(1);
        self.total_size.div_ceil(a) * a
    }
}

/// A `stlt`: `Data` + `Metadata`.
#[derive(Debug)]
pub struct StltAsset {
    format: FormatId,
    key: ValueKey,
    layout: Layout,
    guid: u64,
}

impl StltAsset {
    /// The decoded layout.
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The asset guid.
    pub fn guid(&self) -> u64 {
        self.guid
    }
}

impl Metadata for StltAsset {
    fn name(&self) -> Option<&str> {
        self.layout.name.as_deref()
    }
    fn kind(&self) -> Option<&str> {
        Some("stlt")
    }
    fn guid(&self) -> Option<u64> {
        Some(self.guid)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Vec::new()
    }
}

impl Data for StltAsset {
    fn format(&self) -> &FormatId {
        &self.format
    }
    fn key(&self) -> &ValueKey {
        &self.key
    }
    fn as_metadata(&self) -> Option<&dyn Metadata> {
        Some(self)
    }
}

#[cfg(test)]
mod tests;
