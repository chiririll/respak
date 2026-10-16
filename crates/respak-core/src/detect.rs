//! Detection: score functions that decide which format an input is (PROJECT.md §5.3).

use async_trait::async_trait;

use crate::error::Result;
use crate::mediator::{Cx, Target};
use crate::value::{Value, Version};

/// A positive detection result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// Convention: matched magic ≈ 10, each further confirming field ≈ 1–5, extension alone ≈ 1.
    pub score: u32,
    /// Detected version including the layout variant (`Version.minor`).
    pub version: Option<Version>,
}

/// What a score function sees. The header is read once by the mediator and shared.
pub struct Probe<'a> {
    /// The value being identified.
    pub source: &'a Value,
    /// Lower-case extension of the source's path, without the dot.
    pub extension: Option<&'a str>,
    /// Length from `stat`, if known.
    pub len: Option<u64>,
    /// The first bytes of the source, as many as the largest `header_len()` among candidates
    /// (fewer if the source is shorter).
    pub header: &'a [u8],
}

/// The score half of a read edge.
#[async_trait]
pub trait Detect: Send + Sync {
    /// `None` = not this format.
    async fn score(&self, input: &Probe<'_>, cx: &Cx) -> Result<Option<Detected>>;

    /// How many leading bytes `score` wants in [`Probe::header`].
    fn header_len(&self) -> usize {
        0
    }
}

/// A read edge: turns a generic value (e.g. `file`) into a specific one (e.g. `png`).
#[async_trait]
pub trait ReadHandler: Detect {
    /// Reads `source` as the format that `detected` scored.
    async fn read(&self, source: Value, detected: &Detected, cx: &Cx) -> Result<Value>;
}

/// A convert edge: turns a value of one format (or capability) into another format.
#[async_trait]
pub trait ConvertHandler: Send + Sync {
    /// Converts `source` to `target.format`.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value>;
}

/// Score helper: the little-endian `u16` at `offset`, if the header is long enough.
pub fn u16le(header: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(
        header.get(offset..end)?.try_into().ok()?,
    ))
}

/// Score helper: the little-endian `u32` at `offset`, if the header is long enough.
pub fn u32le(header: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(
        header.get(offset..end)?.try_into().ok()?,
    ))
}

/// Score helper: whether `value` lies in `range` (a missing value is outside every range).
pub fn within<T: PartialOrd>(value: Option<T>, range: std::ops::RangeInclusive<T>) -> bool {
    value.is_some_and(|v| range.contains(&v))
}

/// Score helper: whether `header[offset..]` starts with `magic`.
pub fn magic(header: &[u8], offset: usize, magic: &[u8]) -> bool {
    offset
        .checked_add(magic.len())
        .and_then(|end| header.get(offset..end))
        == Some(magic)
}
