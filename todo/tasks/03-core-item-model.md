# 03 · Core: item model & ids
**Status:** todo · **Depends on:** 02 · **See:** PROJECT.md §6, §7

## Goal
Define the basic identity and data types shared by all plugins.

## Scope (`src/item.rs`, `src/version.rs`)
- `ItemRef { file: String, key: String }`: the file plus the format's internal id. It is `Serialize/Deserialize`, `Hash` and `Eq`. Document examples: an RPak GUID as hex, a VPK entry path, empty for a single-asset file.
- `RequestKey`: an opaque, hashable identity used for dedup and cycle detection. It is built from the request type name plus canonical content; for `serde_json::Value`, keys are sorted.
- `VersionRange`: inclusive, with an open end. Constructors `exact`, `from`, `between`; method `contains`.
- `RawItem { item: ItemRef, kind: String, version: u32, header: Bytes, data: ItemData }`.
- `ItemData`: a lazy byte source. It wraps `Arc<dyn ItemDataSource>` with `len()` and `async read(range) -> Result<Bytes>`. **It must never require the full payload in memory.** Document the rule from PROJECT.md §7.
- `DiscoveredItem { select: Value, kind, version, name: Option<String>, targets: Vec<TargetInfo> }` and `TargetInfo { type_id, label, defaults: Value }`.
- All bodies with logic are `todo!()`. Every public item gets a doc comment.

## Acceptance
- It compiles, and `cargo doc` renders every type with its contract.
