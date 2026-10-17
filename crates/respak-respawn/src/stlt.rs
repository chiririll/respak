//! `stlt`: a settings layout, read from `rpak.asset`. Apex Legends, asset version 0 (72-byte
//! header). It names and types the fields of the value buffer of a `stgs`.
//!
//! Ported from rsx `src/game/rtech/assets/settings_layout.{h,cpp}` (`SettingsLayoutHeader_v0_t`,
//! `SettingsField_t`, `SettingsLayoutAsset`). Read edge only: the layout is decoded into a tree
//! ([`Layout`]); the `stgs` reader uses it to decode its values.

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
use crate::rpak::{RpakAsset, MEM};

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

/// Read edge `rpak.asset -> stlt`.
pub struct StltRead;

#[async_trait]
impl Detect for StltRead {
    /// The asset kind is `stlt`; any asset version other than 0 is `UnsupportedVersion` when read.
    async fn score(&self, input: &Probe<'_>, _cx: &Cx) -> Result<Option<Detected>> {
        let Some(asset) = input.source.downcast_ref::<RpakAsset>() else {
            return Ok(None);
        };
        Ok((asset.entry.kind == *b"stlt").then(|| Detected {
            score: 10,
            version: Some(Version::new(asset.entry.version)),
        }))
    }
}

/// A header's pointers as `.mem` offsets. The asset header is relocated by the volume; a
/// sub header lives in `.mem`, where a pointer is still the stored `{page, offset}`.
struct Pointers<'a> {
    asset: &'a RpakAsset,
    relocated: bool,
}

impl Pointers<'_> {
    fn get(&self, h: &Reader<'_>, at: usize) -> Result<Option<u64>> {
        let slot: [u8; 8] = h.array(at)?;
        if self.relocated {
            let v = u64::from_le_bytes(slot);
            Ok((v != 0 && v != u64::MAX).then_some(v))
        } else {
            // A pointer in `.mem` that was never relocated is all zeros, which would otherwise
            // read as page 0, offset 0.
            Ok((slot != [0; 8])
                .then(|| self.asset.mem_offset_of(slot))
                .flatten())
        }
    }
}

async fn cstr(asset: &RpakAsset, at: u64, mem_len: u64) -> Option<String> {
    if at >= mem_len {
        return None;
    }
    let end = (at + 512).min(mem_len);
    let bytes = asset.read_mem(at, end - at).await.ok()?;
    let nul = bytes.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&bytes[..nul]).into_owned())
}

fn text(names: &[u8], at: usize) -> String {
    let tail = names.get(at..).unwrap_or_default();
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    String::from_utf8_lossy(&tail[..end]).into_owned()
}

/// Reads the layout whose 72-byte header is `header`.
fn read_layout<'a>(
    asset: &'a RpakAsset,
    header: bytes::Bytes,
    relocated: bool,
    mem_len: u64,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<Layout>> + Send + 'a>> {
    Box::pin(async move {
        if depth > DEPTH {
            return Err(Error::corrupt("stlt: sub layouts nest too deep"));
        }
        let h = reader(&header);
        let p = Pointers { asset, relocated };
        let name_at = p.get(&h, 0x00)?;
        let field_data = p.get(&h, 0x08)?;
        let field_map = p.get(&h, 0x10)?;
        let table_size = h.u32(0x18)?;
        let field_count = h.u32(0x1C)?;
        let extra_data_size_index = h.u32(0x20)?;
        let element_count = h.i32(0x2C)?;
        let total_size = h.u32(0x30)?;
        let string_data = p.get(&h, 0x38)?;
        let sub_headers = p.get(&h, 0x40)?;
        if table_size > LIMIT || field_count > LIMIT {
            return Err(Error::corrupt(format!(
                "stlt: implausible table of {table_size} slots / {field_count} fields"
            )));
        }

        let name = match name_at {
            Some(at) => cstr(asset, at, mem_len).await,
            None => None,
        };

        let table = match field_data {
            Some(at) if table_size > 0 => {
                let len = u64::from(table_size) * 8;
                if at.saturating_add(len) > mem_len {
                    return Err(Error::corrupt(
                        "stlt: the field table is outside the pak's data",
                    ));
                }
                asset.read_mem(at, len).await?
            }
            _ => bytes::Bytes::new(),
        };
        let map = match field_map {
            Some(at) if field_count > 0 => {
                let len = u64::from(field_count) * 4;
                if at.saturating_add(len) > mem_len {
                    return Err(Error::corrupt(
                        "stlt: the field map is outside the pak's data",
                    ));
                }
                asset.read_mem(at, len).await?
            }
            _ => bytes::Bytes::new(),
        };
        let names = match string_data {
            Some(at) if at < mem_len => asset.read_mem(at, NAMES_WINDOW.min(mem_len - at)).await?,
            _ => bytes::Bytes::new(),
        };

        // Alignment and the sub layout count come from every slot, empty ones included.
        let mut alignment = 1;
        let mut highest = 0u8;
        let slots = reader(&table);
        for i in 0..table_size as usize {
            let ty = slots.u16(i * 8)?;
            if let Some(a) = FieldType::from_u16(ty).and_then(FieldType::alignment) {
                alignment = alignment.max(a);
            }
            highest = highest.max(slots.get(i * 8 + 7, 1)?[0]);
        }

        let entries = reader(&map);
        let mut fields = Vec::with_capacity(field_count as usize);
        for i in 0..field_count as usize {
            let bucket = entries.u16(i * 4)? as usize;
            let help = entries.u16(i * 4 + 2)? as usize;
            if bucket >= table_size as usize {
                return Err(Error::corrupt(format!(
                    "stlt: field {i} is in bucket {bucket}"
                )));
            }
            let raw_ty = slots.u16(bucket * 8)?;
            let ty = FieldType::from_u16(raw_ty)
                .ok_or_else(|| Error::corrupt(format!("stlt: field type {raw_ty}")))?;
            let tail = slots.u32(bucket * 8 + 4)?;
            fields.push(Field {
                name: text(&names, usize::from(slots.u16(bucket * 8 + 2)?)),
                help: text(&names, help),
                ty,
                offset: tail & 0x00FF_FFFF,
                sub: (tail >> 24) as u8,
            });
        }
        fields.sort_by_key(|f| f.offset);

        let mut subs = Vec::new();
        if let Some(at) = sub_headers {
            let count = usize::from(highest) + 1;
            let len = (count * HEADER_LEN) as u64;
            if at.saturating_add(len) > mem_len {
                return Err(Error::corrupt(
                    "stlt: the sub layouts are outside the pak's data",
                ));
            }
            let block = asset.read_mem(at, len).await?;
            for chunk in block.chunks_exact(HEADER_LEN) {
                subs.push(
                    read_layout(asset, block.slice_ref(chunk), false, mem_len, depth + 1).await?,
                );
            }
        } else if fields.iter().any(|f| f.sub != 0) {
            return Err(Error::corrupt(
                "stlt: a field names a sub layout but there are none",
            ));
        }

        Ok(Layout {
            name,
            fields,
            element_count,
            total_size,
            extra_data_size_index,
            alignment,
            subs,
        })
    })
}

#[async_trait]
impl ReadHandler for StltRead {
    /// Parses the header, the fields and the sub layouts.
    async fn read(&self, source: Value, _detected: &Detected, _cx: &Cx) -> Result<Value> {
        let asset = source.downcast_ref::<RpakAsset>().ok_or_else(|| {
            Error::new(ErrorKind::Custom {
                source: None,
                message: "stlt must be read from an rpak.asset".into(),
            })
        })?;
        if asset.entry.kind != *b"stlt" {
            return Err(Error::new(ErrorKind::Custom {
                source: None,
                message: "the asset is not a stlt".into(),
            }));
        }
        if asset.entry.version != 0 {
            return Err(Error::new(ErrorKind::UnsupportedVersion {
                format: "stlt".into(),
                version: asset.entry.version.to_string(),
            }));
        }
        let header = asset.read_header().await?;
        if header.len() != HEADER_LEN {
            return Err(Error::corrupt(format!(
                "stlt header is {} bytes, expected {HEADER_LEN}",
                header.len()
            )));
        }
        let mem_len = asset.volume.stat(MEM).await?.content_length();
        let layout = read_layout(asset, header, true, mem_len, 0).await?;
        Ok(Arc::new(StltAsset {
            format: FormatId::new("stlt").with_version(Version::new(0)),
            key: source.key().clone(),
            layout,
            guid: asset.entry.guid,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stride_rounds_up_to_the_alignment() {
        let layout = |total_size, alignment| Layout {
            name: None,
            fields: Vec::new(),
            element_count: 1,
            total_size,
            extra_data_size_index: 0,
            alignment,
            subs: Vec::new(),
        };
        assert_eq!(layout(12, 8).stride(), 16);
        assert_eq!(layout(16, 8).stride(), 16);
        assert_eq!(layout(5, 1).stride(), 5);
        assert_eq!(layout(0, 0).stride(), 0);
    }

    #[test]
    fn names_stop_at_nul_and_survive_a_bad_offset() {
        let names = b"\0abc\0de";
        assert_eq!(text(names, 1), "abc");
        assert_eq!(text(names, 5), "de");
        assert_eq!(text(names, 99), "");
    }
}
