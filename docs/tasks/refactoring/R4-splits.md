# R4 · Large file splits
**Status:** done · **Risk:** low–medium · **Depends on:** R1 (except where noted)

Per file: move tests/fixtures out of `src` first, then split, keeping public paths via `pub use`.

## R4.1 `respak-formats/src/cast.rs` (1380 lines) (M)
~565 lines are an inline `mod tests` with its own parser helpers. Production mixes options/selectors, the `Node` tree + `CastWriter`, model encode, animation encode and math. Split into `cast/{options,node,model,anim}.rs`; tests into `cast/tests.rs` or `tests/cast_*.rs`, with a shared test reader.

## R4.2 `respak-rad/src/mbnk.rs` (961 lines) (M)
1. Move `pub mod fixtures` (used by `e2e_tests.rs`) out of the production file (`src/testing.rs` behind `#[cfg(test)]`, or `tests/support/`).
2. Move `e2e_tests.rs` to `crates/respak-rad/tests/e2e.rs`.
3. Split production into `mbnk/{layout,entry,bank,source,handler}.rs`.

## R4.3 `respak-respawn` testing → dev-crate (M)
Decided: separate dev-crate (working name `respak-respawn-testing`). `testing.rs` (1615 lines) is compiled into the lib behind the `testing` feature with a blanket `allow(missing_docs, dead_code)`.
- Users: `respak-cli/tests/{dump,material_cli,names}.rs`, `integration/tests/{aseq,material,rmdl}.rs`, respawn's own tests.
- Split by concern as it moves: generic pak/patch builders, texture/uimg builders, `PakBuilder` (matl, shds, dtbl), `rmdl` builders, `aseq` builders.
- Dedupe the `put16/put32/put_f` helpers, the `le32/le64` closures in `rtech.rs:740`, and the `put`/`cstr` helpers (`testing.rs:1469`, `tests/starpak.rs:71`).
- Delete unused builders instead of keeping `allow(dead_code)`. Remove the `testing` feature from respawn.
- Watch for a dependency cycle: the dev-crate needs respawn's public types, and respawn's in-crate `#[cfg(test)]` code can't use a crate that depends on respawn. Move those tests to `tests/` if needed.

## R4.4 `respak-respawn/src/rmdl.rs` (1901 lines, ~1420 production) (M–L)
Split into `rmdl/{block,header,studio,vg,asset,handler}.rs`. Break `decode_mesh` (182 lines) into one function per vertex stream; split `Studio::parse` (133 lines) and `RmdlHeader::parse` (123 lines). Move `fix_offset` next to `B::fixed`. If R1.3 is done, `block` is the shared reader.

## R4.5 `respak-respawn/src/rpak.rs` (1196 lines) (M)
Split into `rpak/{volume,body,progress,names}.rs` with the oodle/rtech progressive readers as separate files. Check the patch-name stem parsing (`rpak.rs:444`) against `patch.rs`; the same naming may be parsed twice.

## R4.6 Respawn module layout (S–M)
- `<fmt>_json.rs` files (`matl`, `stgs`, `stlt`, `uiia`, `uimg`, `rmdl`) become submodules of their format; `dtbl_text.rs` likewise.
- `hex(guid)` is in `matl_json.rs:66` but used by `aseq.rs:566`: move to `guid.rs`.
- Group the 33 flat `mod` lines in `lib.rs` into `container/`, `assets/`, `codec/`.
- `aseq.rs:473` and `asqd.rs:88` both define `read_upto`; unify with the `dtbl.rs` cstr/string reader.

## Done
- R4.1: `cast/{options,node,model,anim,tests}.rs`.
- R4.2: `mbnk/{layout,entry,bank,source,handler,fixtures,tests}.rs`. Deviation: `e2e_tests.rs` stays in `src/` — the fixtures need private layout internals, so it cannot move to `tests/` without making them public.
- R4.3: new crate `respak-respawn-testing` (no dependency on respawn, so no cycle); `testing` feature and `testing.rs` removed. `put*`/`cstr` helpers deduped into `bytes.rs`; unused `page1_at` deleted. The crate allows `missing_docs` explicitly in its manifest instead of a blanket `allow(dead_code)`. The `le32/le64` closures in `rtech.rs` and `tests/starpak.rs:put` turned out to be unrelated to the builders and were left alone.
- R4.4: `rmdl/{block,header,studio,vg,asset,handler,tests}.rs`; `decode_mesh` and `Studio::parse` broken into per-part functions; `fix_offset` lives next to `FixedOffset`.
- R4.5: `rpak/{body,rtech_read,oodle_read,volume,chain,master,names,data,handler,tests}.rs`. The patch file naming (`stem(NN).rpak`) was parsed twice in `rpak.rs`; now `names.rs`. `patch.rs` has no naming code.
- R4.6: `*_json.rs` and `dtbl_text.rs` are submodules (`json.rs`, `text.rs`); `hex` is in `guid.rs`; modules grouped into `container/`, `codec/`, `assets/` (lib.rs re-imports them by name, so `crate::rpak::...` paths are unchanged). The two `read_upto` are unified as `mem::read_clamped`; the cstr readers differ in error semantics and were left.
- Extra: `aseq.rs` (1014 production lines) split into `aseq/{options,header,desc,asset,handler}.rs`.
- Left to R5: `snowflake`, `rtech`, `anim_codec`, and the large test files.
