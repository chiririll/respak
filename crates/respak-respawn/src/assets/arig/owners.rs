//! Which `arig` or model owns an `aseq`.

use super::*;

/// The skeleton owner of the `aseq` `aseq_guid` of the pak at `origin` (a virtual path): the first
/// `arig` that lists it, else the first `mdl_` (rsx order; the `arig` of a pak wins over its
/// models). Both values have the `Skeleton` capability. Scans only the assets of the pak and
/// `common.rpak` next to it, once per pak; none found is `MissingDependency`.
pub(crate) async fn owner_of(
    aseq_guid: u64,
    origin: &str,
    owners: &Owners,
    cx: &Cx,
) -> Result<Value> {
    let index = owners.index(origin, cx).await?;
    let Some((path, kind)) = index.get(&aseq_guid) else {
        return Err(Error::new(ErrorKind::MissingDependency {
            item: format!("the rig of aseq {}", hex(aseq_guid)),
            file: format!(
                "no arig or mdl_ in {} lists it (set `rig` to name one)",
                rig_paks(origin).join(" or ")
            ),
        }));
    };
    owners.rig(path, kind, cx).await
}

/// The pak at `origin` and the `common.rpak` next to it, the paks whose rigs are looked at.
pub(super) fn rig_paks(origin: &str) -> Vec<String> {
    let dir = origin
        .rsplit_once('/')
        .map_or(String::new(), |(d, _)| format!("{d}/"));
    let mut paks = vec![origin.to_owned()];
    let common = format!("{dir}common.rpak");
    if common != origin {
        paks.push(common);
    }
    paks
}

/// For each `aseq` guid, the path and the kind (`arig` or `rmdl`) of the rig that owns it.
pub(super) type OwnerIndex = HashMap<u64, (String, &'static str)>;

/// The rigs of the paks, by the sequences they list. A pak is indexed once, on the first sequence
/// whose rig is not in the plugin cache, instead of being searched again for every sequence.
#[derive(Default)]
pub(crate) struct Owners {
    indexes: std::sync::Mutex<HashMap<String, Arc<OnceCell<Arc<OwnerIndex>>>>>,
    /// The rigs already read, by path: reading one parses its bones and looks at its sibling models.
    rigs: std::sync::Mutex<HashMap<String, Arc<OnceCell<Value>>>>,
}

impl Owners {
    /// The index for the sequences of the pak at `origin`: the first `arig` that lists a sequence,
    /// else the first `mdl_` (the `arig` of a pak wins over its models, the pak over `common.rpak`).
    async fn index(&self, origin: &str, cx: &Cx) -> Result<Arc<OwnerIndex>> {
        let cell = self
            .indexes
            .lock()
            .expect("the index map is not poisoned")
            .entry(origin.to_owned())
            .or_default()
            .clone();
        let index = cell.get_or_try_init(|| build_index(origin, cx)).await?;
        Ok(index.clone())
    }

    /// The rig at `path`, opened and read once.
    async fn rig(&self, path: &str, kind: &'static str, cx: &Cx) -> Result<Value> {
        let cell = self
            .rigs
            .lock()
            .expect("the rig map is not poisoned")
            .entry(path.to_owned())
            .or_default()
            .clone();
        let rig = cell
            .get_or_try_init(|| async {
                let asset = cx.mediator.open(path, cx).await?;
                as_rig(asset, kind, cx).await
            })
            .await?;
        Ok(rig.clone())
    }
}

pub(super) async fn build_index(origin: &str, cx: &Cx) -> Result<Arc<OwnerIndex>> {
    let mut index = OwnerIndex::new();
    for pak in &rig_paks(origin) {
        let file = match cx.mediator.open(pak, cx).await {
            Ok(f) => f,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound { .. }) => continue,
            Err(e) => return Err(e),
        };
        let container = cx.mediator.mount(file, cx).await?;
        let Some(c) = container.as_container() else {
            continue;
        };
        for suffix in [".arig", ".mdl_"] {
            for name in list_assets(&c.volume(), suffix).await? {
                cx.check_cancelled()?;
                let asset = c.entry(&name).await?;
                if let Some((kind, guids)) = sequences_of(&asset).await {
                    let path = asset.key().to_string();
                    for guid in guids {
                        index.entry(guid).or_insert_with(|| (path.clone(), kind));
                    }
                }
            }
        }
    }
    Ok(Arc::new(index))
}

/// Reads an `arig` / `mdl_` asset (`kind` `arig` or `rmdl`) as a value with a skeleton.
pub(super) async fn as_rig(asset: Value, kind: &'static str, cx: &Cx) -> Result<Value> {
    let rig = cx
        .mediator
        .identify_as(asset, &FormatId::new(kind), cx)
        .await?;
    if rig.as_skeleton().is_none() {
        return Err(Error::new(ErrorKind::Custom {
            source: None,
            message: format!("`{}` has no skeleton", rig.format()),
        }));
    }
    Ok(rig)
}

/// The rig an explicit `rig` option names: `0x...` is a guid, anything else a path hashed with
/// `string_to_guid`; an `arig` first, else a `mdl_`. Looked for from the pak at `origin`.
pub(crate) async fn explicit_rig(name: &str, origin: &str, cx: &Cx) -> Result<Value> {
    let guid = name
        .strip_prefix("0x")
        .or_else(|| name.strip_prefix("0X"))
        .and_then(|digits| u64::from_str_radix(digits, 16).ok())
        .unwrap_or_else(|| string_to_guid(name));
    for (kind, format) in [("arig", "arig"), ("mdl_", "rmdl")] {
        match locate(guid, kind, origin, cx).await {
            Ok(asset) => return as_rig(asset, format, cx).await,
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::NotFound { .. } | ErrorKind::MissingDependency { .. }
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Err(Error::new(ErrorKind::MissingDependency {
        item: format!("rig `{name}`"),
        file: format!("no arig or mdl_ {} (searched from {origin})", hex(guid)),
    }))
}

/// The file names in a pak volume that end in `suffix`, in listing order.
pub(super) async fn list_assets(volume: &Operator, suffix: &str) -> Result<Vec<String>> {
    Ok(volume
        .list("/")
        .await?
        .into_iter()
        .filter(|e| e.metadata().is_file() && e.name().ends_with(suffix))
        .map(|e| e.name().to_owned())
        .collect())
}

/// The kind to read `asset` as (`arig` or `rmdl`) and the `aseq` guids it lists, if it is an
/// `arig` / `mdl_` pak asset. Reads only the asset header and its guid list.
pub(super) async fn sequences_of(asset: &Value) -> Option<(&'static str, Vec<u64>)> {
    let asset = asset.downcast_ref::<RpakAsset>()?;
    let header = asset.read_header().await.ok()?;
    let (kind, list) = match &asset.entry.kind {
        b"arig" => (
            "arig",
            ArigHeader::parse(asset.entry.version, &header)
                .ok()?
                .anim_seqs,
        ),
        b"mdl_" if (16..=20).contains(&asset.entry.version) && header.len() == 96 => {
            let b = reader(&header);
            let at = b.u64(0x50).ok()?;
            (
                "rmdl",
                (
                    (at != 0 && at != u64::MAX).then_some(at),
                    u32::from(b.u16(0x4A).ok()?),
                ),
            )
        }
        _ => return None,
    };
    Some((kind, read_guids(asset, list.0, list.1).await.ok()?))
}

/// The version of the model `name` (the first in the pak at `origin`), if it is of a supported kind:
/// the studiohdr layout the rig is expected to share (rsx behaviour).
pub(super) async fn sibling_model(name: &str, origin: &str, cx: &Cx) -> Option<Version> {
    let model = cx
        .mediator
        .open(&format!("{origin}/{name}"), cx)
        .await
        .ok()?;
    let m = model.downcast_ref::<RpakAsset>()?;
    let header = m.read_header().await.ok()?;
    let studio_at = reader(&header).u64(0).ok()?;
    let source_filename_offset = m
        .read_mem(studio_at, 0xDA)
        .await
        .ok()
        .and_then(|b| reader(&b).u16(0xD8).ok());
    rmdl::variant(m.entry.version, m.pak_created_time, source_filename_offset).ok()
}
