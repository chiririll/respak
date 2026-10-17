# R5 · Riskier work
**Status:** todo · **Risk:** medium · **Depends on:** R2, R4

## R5.1 Shared blocking-stream helper (M) · done: `respak_core::stream::blocking_stream`
`miles/asrc.rs` and `nihav/bik.rs` hand-roll the same blocking-pool decode → `mpsc` → `blocking_send` → cancel check → `Err(Cancelled)` last item. Extract e.g. `respak_core::stream::blocking_stream(cancel, |emit| ..)`. Concurrency code: run the `bik.rs` cancel tests and `formats/tests/cancel.rs` first.

## R5.2 Codec splits (M each, only with the vector tests green)
- ✅ done (`codec/snowflake/`, longest fn `run` 160 lines; real-block hashes in an ignored test, run with `SNOWFLAKE_VECTORS=<dir of NNNN.in>`): `snowflake.rs` (1334 lines): `run` (~440 lines), `literals` (~185), `c880` (~159); 95 `as` casts and a 0x25000 flat state block addressed by raw offsets. Move the SIMD constants to a table, name the state offsets (accessors or a `#[repr]` struct), split into `snowflake/{state,bits,model,literals,run}.rs`.
- ✅ done: `rtech.rs` is now `codec/rtech/{mod,lut,decoder,tests}.rs`; `run` and `decode` are split into short helpers (the real-file `ui_rpak` tests skip when the local game file is missing).
- ✅ done (`stgs` `read` → `read_values/read_mod_names/read_mods/decode_settings`, `uiia` `read` → `pick_quality`, `oodle_read` → `scan_range/decode_segments/assemble`). Also split the read handlers (plus tests) of `uiia`, `stgs`, `uimg`, `stlt`, `dtbl`, `txtr` into `read.rs`/`tests.rs`, `uiia/pixels.rs`, `pak/tables.rs`, `arig/owners.rs`, `anim_codec/tests.rs`.
- ✅ done: the repeated downcast + kind check and `Detect::score` of the 11 rpak asset readers are now `rpak::rpak_asset(&source, kind)` and `rpak::detect_kind` / `detect_kind_by`.

## R5.3 Core public API (M, breaking is OK)
Decided: breaking the `respak-core` public API is acceptable. Update `respak-respawn`, `-formats`, `-miles`, `-nihav`, `-steam`, `-cli` in the same step.
- `Mediator::new/with_events/with_middleware`, `Cx::new/with_*` and `Mediator::publish/subscribe/detect` are public next to `Engine::builder`. Make them `pub(crate)`, or hide them behind the testing helpers. Check `tests/common/mod.rs` first.
- Clarify `identify` / `identify_as` / `detect` / `scan` (`mediator.rs:110-213`): rename or document.
- `Engine::open` vs `Mediator::open` vs `Engine::expand` overlap; settle on distinct names. `Engine::formats/events` duplicate `Mediator` accessors.
- `FormatInfo` (graph) and `FormatDef` (plugin) are near-duplicates; `OutEdge`/`EdgeInfo`/`Step` overlap. Derive them from the registry in one place (`graph.rs:261`).

## R5.4 Test layout (M, after R5.3)
- Move the `tests/common/mod.rs` fixtures (204 lines, `allow(dead_code)`) into a `respak-core` testing module; reuse them in `cache.rs` (`Versioned`) and `linking.rs` (`SlowPlugin`), and in downstream crates.
- Split the biggest test files by concern: `respak-core/tests/{containers,graph,events,engine}.rs`, `respak-respawn/tests/aseq.rs` (1386 lines) and `rpak.rs` (655). Keep pure planning tests as unit tests next to `graph.rs`; dedupe against `src/graph.rs:314`.
- Look for repeated fixture builders in `formats/tests/*` and add `tests/common/mod.rs` if so.


# User questions

- ~~move bik into miles and rename the crate to rad~~ Done: `bik`, `bik_decode` and the bik tests live in `respak-rad` (was `respak-miles`), plugin `RadPlugin` (id `rad`); `respak-nihav` keeps only the encoders and muxers.