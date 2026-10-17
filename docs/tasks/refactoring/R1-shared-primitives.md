# R1 · Shared primitives
**Status:** done · **Risk:** low

## R1.1 Shared error helpers (S) — done (MAX_* shared limit check deferred)
`corrupt(context)` is copy-pasted ~19 times in `respak-respawn` (`anim_codec`, `arig`, `aseq`, `asqd`, `dtbl`, `matl`, `pak`, `patch`, `rmdl`, `rpak`, `shader`, `starpak`, `stgs`, `stlt`, `txtr`, `uiia`, `uimg`) and 5 more times elsewhere (`formats/lib.rs`, `formats/bc.rs`, `miles/lib.rs`, `nihav/bik.rs`, `nihav/bik_decode.rs`). `unsupported` has three signatures (`bc.rs`, `formats/lib.rs`, `bik_decode.rs`) and `dxbc.rs` prefixes "DXBC: ".
- Add `Error::corrupt(format_id, ctx)`, `Error::unsupported(..)` and `Error::is_cancelled()` to `respak-core`. Replace `cast.rs` `is_cancelled` and the inline matches in tests.
- Replace the per-file copies. Rename `nihav/enc.rs` `bad` (too vague). Keep one-off helpers (`custom`, `storage`, `not_found`) only if they add something.
- Check the `MAX_*` limit constants in `rmdl.rs:64-68` and similar; a shared `limit` check may fit here.

## R1.2 `Error::msg` (S) — done for the core sites (typed kinds not added)
`ErrorKind::Custom { source: None, message }` is built by hand at `mediator.rs:536`, `plugin.rs:131`, `engine/convert.rs:133`, `engine/mod.rs:104`, `value/lazy.rs:103,168`. Add `Error::msg(impl Into<String>)`. Consider typed kinds for the registry validation (`plugin.rs:131`) and missing-operator (`engine/mod.rs:104`) cases.

## R1.3 Shared checked byte reader (M) — done (module is `reader`, not `bytes`)
Hand-rolled little-endian readers: `core/detect.rs:58-72`, `miles/mbnk.rs:197-214`, `nihav/bik.rs:81`, `formats/bc.rs:93-115`, `formats/cast.rs:829` (tests), `miles/e2e_tests.rs`, and in respawn `dxbc.rs`, `matl.rs:88-100`, `pak.rs:62-109`, `rmdl.rs:136-200` (`B`), `uimg.rs`, `stlt.rs`, `dtbl.rs`, `rtech.rs`.
- New module in `respak-core` (`bytes`): checked `Reader`/`Cursor`, `u8/u16/u32/u64/i16/i32/i64/f32/cstr`, returning `Result`, plus an `Option` variant for detection. Use `rmdl::B` as the base.
- Rewrite `detect::u16le/u32le` on top of it.
- Each format keeps a thin wrapper mapping to its own `corrupt`.

## R1.4 Migrate parsers to the reader (M) — done in one pass, except the items listed below
One crate per commit, in this order: `miles`, `nihav`, `formats` (`bc`), `respawn` (`pak` first, then `matl`, `dxbc`, `rmdl`, cstr readers in `uimg`/`stlt`/`dtbl`).
Left as is: `bc.rs` (`chunks_exact` already guarantees the lengths), the Option-returning cstr readers in `uimg`/`stlt`/`dtbl` (different semantics from `Reader::cstr`), `rtech.rs` and test helpers.

## R1.5 Remove panic risks on untrusted data (S–M) — done except the `as usize` sweep and the optional lint (see notes)
- `pak.rs:99-109` `le_*` index directly; `matl.rs:88` `at + N` can overflow; `B::fixed` (`rmdl.rs:195`) adds without `checked_add`.
- `bcf1.rs`, `mstr.rs`, `rada.rs`: `u32le(..).expect("length checked")` per field. Read the fixed header once through the reader.
- Verify `videofile.rs:234` (`tracks[t as usize]`) is validated first.
- Replace the many `as usize` in readers with `usize::try_from` where the value comes from the file.
- Add truncated-input tests per parser touched.
- Optional: warn on `clippy::unwrap_used`/`expect_used` in `[workspace.lints]`, allowed in tests.

Notes (R1.5): `videofile.rs:234` was already guarded by the bounds check above it. Truncation tests added for `bcf1`, `mstr`, `rada`, `matl` and the rpak container. `unwrap_used`/`expect_used` lint not enabled: the production code still has ~70 `expect`s (mostly lock poisoning and length-checked slices), so it needs its own pass. `as usize` on file-supplied values was not swept.
