//! `patch_master.rpak`: which patch is the newest of a pak.

use opendal::Operator;
use respak_core::CancellationToken;

use super::body::open_body;
use super::names::patch_file_name;
use crate::pak::{PagePtr, Tables};

/// The path of the newest patch of `path` that a `patch_master.rpak` in its directory names, if
/// there is one and `path` is not a patch itself. Anything wrong with the master means no patch.
pub(super) async fn newest_patch(
    op: &Operator,
    path: &str,
    cancel: &CancellationToken,
) -> Option<String> {
    let (dir, name) = path
        .rsplit_once('/')
        .map_or(("", path), |(d, f)| (&path[..d.len() + 1], f));
    let stem = name.strip_suffix(".rpak")?;
    if stem.eq_ignore_ascii_case("patch_master") || stem.ends_with(')') {
        return None;
    }
    let entries = patch_master(op, &format!("{dir}patch_master.rpak"), cancel).await?;
    let (_, number) = entries
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))?;
    if number == 0 {
        return None;
    }
    let patch = format!("{dir}{}", patch_file_name(stem, u32::from(number)));
    op.stat(&patch).await.ok().map(|_| patch)
}

/// The entries of the `Ptch` asset of a patch master: pak file name and the number of its newest
/// patch. The asset header is `i32 unk, i32 count, ptr names, ptr versions`: `names` points at
/// `count` pointers to NUL-terminated strings, `versions` at `count` bytes.
async fn patch_master(
    op: &Operator,
    path: &str,
    cancel: &CancellationToken,
) -> Option<Vec<(String, u8)>> {
    let (header, body) = open_body(op, path, 0, None, cancel).await.ok()?;
    if header.patch_count != 0 {
        return None;
    }
    let tables = Tables::parse(&header, &body.read(0..header.tables_len()).await.ok()?).ok()?;
    let asset = tables.assets.iter().find(|a| a.kind_string() == "Ptch")?;
    let mem = |at: u64, len: u64| {
        let (body, base) = (&body, tables.page_data_offset + at);
        async move { body.read(base..base + len).await.ok() }
    };
    let ptr = |b: &[u8]| {
        let p = PagePtr {
            page: u32::from_le_bytes(b.get(..4)?.try_into().ok()?),
            offset: u32::from_le_bytes(b.get(4..8)?.try_into().ok()?),
        };
        if p.is_null() {
            return None;
        }
        tables.mem_offset(p)
    };
    let head = tables.header_range(asset)?;
    let h = mem(head.start, head.end - head.start).await?;
    let count = u64::from(u32::from_le_bytes(h.get(4..8)?.try_into().ok()?));
    let (names, versions) = (ptr(h.get(8..16)?)?, ptr(h.get(16..24)?)?);
    if count > 4096 {
        return None;
    }
    let pointers = mem(names, count * 8).await?;
    let versions = mem(versions, count).await?;
    let mut out = Vec::new();
    for (i, p) in pointers.chunks_exact(8).enumerate() {
        let at = ptr(p)?;
        let bytes = mem(at, 256.min(tables.mem_len().saturating_sub(at))).await?;
        let name = bytes.split(|&b| b == 0).next()?;
        out.push((
            String::from_utf8_lossy(name).into_owned(),
            *versions.get(i)?,
        ));
    }
    Some(out)
}
