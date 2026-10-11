# respak — project summary

respak is a Rust library, plus a separate CLI, that reads files from Respawn games and converts their contents into usable formats (e.g. an `.rpak` texture to PNG/DDS, a model to Cast), and later back into Respawn formats. It is designed from scratch. The C++ tool **rsx** (`C:\Users\chiririll\Projects\rsx`) is a source of format knowledge only, not code to port.

**Status:** design under discussion, nothing implemented. The task list in [`tasks/`](tasks/README.md) still describes the previous design and will be rewritten once this document is agreed.

---

## 1. Workflow and rules
1. **Interfaces first:** types, traits, crate layout, docs, skeleton plugins and CLI. It compiles; logic bodies are `todo!()`; no tests.
2. **The user reviews the interfaces.**
3. **Test cases are defined together with the user.** Never invent the test suite alone.
4. **Implementation, test-first (TDD).**

Rules:
- **Prefer existing, well-maintained crates.** The one agreed exception is the mediator, hand-written on purpose.
- **Ask the user when something is unclear.** Don't guess.
- **No Steam or downloader code.** Where bytes come from is the host's business.
- **Read only the byte ranges you need** (§6).
- **A format handler only handles its own format.** For anything else it asks the mediator: "here is a value, convert it to format X". Modules may share *data types* of other formats, never call their handlers.
- **Plugin crates depend only on `respak-core`.**
- **Core contains no format knowledge.**
- **A version number is not proof of a layout:** the same version can hide several layouts (§5.3).
- **Keep the public API wrappable for a later FFI:** JSON in/out, object-safe traits.

## 2. Crates
```
crates/
  respak-core/      # mediator, format graph, scoring, capabilities, volumes, Engine, errors
  respak-respawn/   # Respawn formats incl. Respawn's flavours: rpak, starpak, txtr, matl, rmdl, aseq,
                    # arig, dtbl, uimg, shaders, Respawn VPK, … Read now, write later.
  respak-formats/   # common formats: png, dds (read + write); later cast, smd, csv/json, audio
  respak-cli/       # host: registers plugins, CLI commands
# later: respak-miles (mbnk, mstr). Not planned: respak-source (Valve Source MDL/VPK).
```

## 3. Dependencies
| Purpose | Crate | Note |
|---|---|---|
| Storage (input, output, cache) | `opendal` | Pre-1.0: **pin an exact version**. `services::Fs` for disk, `services::Memory` for tests. |
| Async | `tokio`, `tokio-util` (`CancellationToken`), `async-trait`, `futures` (`Shared`) | tokio is a direct dependency of core |
| Data | `serde`, `serde_json`, `bytes` | requests, targets, reports, discovery are JSON |
| Errors / logs | `thiserror`, `tracing` | |
| CLI | `clap` (derive), `anyhow` | CLI crate only |
| Candidates | `globset` (path globs), `moka` (in-memory cache, later) | added when needed |

Rejected, don't reopen without asking: a home-made filesystem replacing OpenDAL; `object_store`; `vfs`; `tower` for the mediator; a message broker; first-match `can_handle` routing (→ scoring); shared intermediate structs such as one `Texture` for every format (→ capabilities).

## 4. Overview
Everything is a **value tagged with a format**. Handlers are **edges** between formats. The mediator detects formats by **scoring probes** and converts by finding the **cheapest path** through the graph. Files are addressed by **virtual paths** that may go into containers, because every container is an **OpenDAL service**.
```
paks/Win64/common.rpak/texture/foo.txtr  →  png

common.rpak is a file → scored as rpak → its volume is mounted → texture/foo.txtr resolved inside it
file(rpak volume, texture/foo.txtr) ─read─▶ txtr ─(Bitmap)─▶ png ─▶ output
   header           read(texture/foo.txtr)                → rpak volume serves the asset header
   permanent mips   convert(rpak.ptr → file)              → page file in the rpak volume
   streamed mips    read(texture/foo.txtr.stream, range)  → mapped onto x.starpak → ranged read on the host
   compressed mip   convert(compressed{oodle,…} → blob)   → codec handler
```
`txtr` never parses rpak structures; `rpak` never learns that `txtr` exists.

## 5. Core architecture

### 5.1 Values and formats
```rust
pub type Value = Arc<dyn Data>;

pub trait Data: Any + Send + Sync + Debug {
    fn format(&self) -> &FormatId;                  // name + resolved version, e.g. txtr@10
    fn key(&self) -> &ValueKey;                     // identity for dedup and cycle detection
    fn approx_size(&self) -> usize { 0 }            // for the later in-memory cache (§7)
    fn as_blob(&self) -> Option<&dyn Blob> { None } // capabilities (§5.4), default: unsupported
    fn as_container(&self) -> Option<&dyn Container> { None }
    fn as_metadata(&self) -> Option<&dyn Metadata> { None }
    fn as_bitmap(&self) -> Option<&dyn Bitmap> { None }
    // … as_mesh, as_skeleton, as_animation, as_table, as_audio
    fn capability(&self, id: TypeId) -> Option<&dyn Any> { None }   // plugin-defined
}
pub struct FormatId { name: Cow<'static, str>, version: Option<Version> }
pub struct Version  { major: u32, minor: u32 }     // minor = layout variant, 0 if none
```
- Formats are names owned by the crate that defines them. Core formats:
  - `file { op: Operator, path, range: Option<Range<u64>> }`: on the host operator or inside a container volume
  - `blob`: bytes in memory
  - `bundle`: several values (one output with several sources)
  - `compressed { codec, data, decoded_len }`: decompressed to `blob` by a plugin's codec handler
- Plugin formats, including internal plumbing: `rpak`, `starpak`, `rpak.ptr`, `rpak.guid`, `txtr`, … (respawn); `png`, `dds`, … (formats).
- **Keys:** a file's key is its full virtual path (+ range); a converted value's key is source key + target.

### 5.2 Edges, registration, paths
- **Read edges** turn a generic value into a specific one (`file → rpak`, `file → txtr`). Each has a probe; the best score wins (§5.3).
- **Convert edges** turn one format into another (`txtr → dds`, `rpak.ptr → file`, `compressed → blob`, `Bitmap → png`). Each has a cost (1 = lossless, higher = lossy) and starts from a format or a capability.
- Registration is grouped per format:
```rust
impl Plugin for RespawnPlugin {
    fn id(&self) -> &str { "respawn" }
    fn register(&self, r: &mut Registry) {
        r.format("rpak")      .from("file", RpakOpen, |p| p.magic(0, b"RPak").required().u16le(4).within(6..=8));
        r.format("starpak")   .from("file", StarpakOpen, |p| p.extension(["starpak"]).required());
        r.format("rpak.ptr")  .to("file", RpakPtrResolve);   // page pointer → page file + range
        r.format("rpak.guid") .to("file", RpakLocate);       // dependency → asset file (own pak, cache, other paks)
        r.format("txtr")
            .from("file", TxtrRead, |p| p.kind("txtr").required().version().within(8..=10))
            .to("dds", TxtrToDds);                           // copies BC blocks, cost 1
    }
}
r.format("png").from("file", PngRead, |p| p.magic(0, b"\x89PNG").required())
               .from_cap::<dyn Bitmap>(PngEncode).cost(2);   // respak-formats
```
- **Path selection:** the graph is built in `Engine::build()`. Cheapest path wins; a direct edge beats a capability edge of equal cost; two equally cheap paths are a **build error**; a request may pin a route with `via`.

### 5.3 Detection by scoring
- **Probes look at header values only.** Layout variants are handled in the handler's code (below).
- Probes use a shared check library: `window(offset, size)`, `magic`, `u16le/u32le/…` with `eq / within / matches(fn)`, `extension`, `len`, and container-provided header values `kind()`, `version()`, `header_size()`. Each check is `.required()` or has `.weight(n)` (default 1).
- **Score:** a failed required check → 0, candidate dropped. Otherwise the sum of passed weights; **highest absolute score wins** (8/10 beats 5/5), the pass ratio only breaks ties. A real tie → `AmbiguousFormat`; the request can set `format`.
- **I/O:** each probe declares its windows; the mediator merges them across candidates and reads each range once.
- **Variants:** one handler per format branches on the variant in code, sampling data if needed (e.g. rsx: `matl` 23.1/23.2 by header size, `rmdl` 12.x by `studiohdr` offsets). The variant becomes `Version.minor`. A separate handler only for big changes, distinguished by its probe. Unknown variant → `UnsupportedVersion` at conversion.
- Discovery reports winner, score and runners-up.

### 5.4 Capabilities
Each format keeps its full native type (`TxtrAsset`, …) and implements the capability traits that apply. Capabilities are lazy views.
- `Blob`: in-memory bytes · `Container`: `volume() -> Operator` · `Metadata`: name, guid, dependencies
- `Bitmap`: dimensions, depth, array/faces, mip count, pixel format, `read_mip(level, layer)`
- `Mesh`, `Skeleton`, `Animation`, `Table`, `Audio`: skeletal, refined with the first real converters
- Core ships owned helpers (e.g. `BitmapBuf`) for converters that create data. Plugins may add capabilities via `Data::capability`.

### 5.5 Mediator, context, linking
```rust
impl Mediator {
    pub async fn convert(&self, source: Value, to: Target, cx: &Cx) -> Result<Value>;  // Target = { format, options: Value }
    pub async fn identify(&self, source: Value, cx: &Cx) -> Result<Value>;             // read edges only
    pub fn publish(&self, e: Event);
    pub fn subscribe(&self) -> broadcast::Receiver<Event>;
}
```
- **One request type** (`Convert`). Open, select, locate, decode and encode are paths, values and edges; plugin-internal needs are private formats, not private requests.
- **Linking:** a converter asks for what it needs (a Cast encoder converts its textures to PNG and writes them via `cx.write`). Dedup produces each thing once; there is no `with_dependencies` flag.
- **Output:** core writes any `Blob` value to the output operator; multi-file or streaming writers use `cx` directly.
- **`Cx`:** mediator handle, call chain of keys, `CancellationToken`, input/output operators, `cache()`, `write(path, value)`, `blocking(..)` (`spawn_blocking`) and `io(..)`.
- **`TypedTarget<T>`** parses target options into a plugin struct (missing fields from `Default`); typed edge adapters downcast `Value` to the handler's type.

### 5.6 Middleware
`async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value>`. Built-ins, in order:
1. **Cancellation:** `Cancelled` if the token is triggered.
2. **Cycle guard:** a key already in the call chain → `DependencyCycle`. Also a **wait-for graph** over in-flight keys (recorded when a request joins an in-flight future), so concurrent chains A→B / B→A can't deadlock.
3. **Dedup:** in-flight map (source key, target) → `Shared` future. Removed on completion, except mounted volumes, which live until the `Engine` call ends.
4. **Tracing** span per request. 5. **Events:** started / finished / failed.

- **No per-request concurrency limit** (a parent awaiting children while holding a permit deadlocks). Limits apply only to top-level outputs and to `cx.blocking` / `cx.io`.
- Results are shared, so `Value` is `Arc` and errors are `Arc`-wrapped (`opendal::Error` isn't `Clone`). Hosts can add middleware.

### 5.7 Events
- `#[non_exhaustive] enum Event`: `RequestStarted`, `RequestFinished`, `RequestFailed`, `InputOpened{path, files}`, `OutputWritten{path}`, `Progress{task, done, total}`, `Warning{message}`, `Custom{source, name, data}`.
- `tokio::sync::broadcast`; `LogSubscriber` forwards to `tracing`. Progress is only an event. Lagging subscribers may miss events; anything that matters is in the returned report.

### 5.8 Writing Respawn formats (later, designed now)
- An rpak writer is an edge `bundle → rpak`. It converts each item to `rpak.asset@V` (writer-side: header, pages, streamed data); per-kind writers register the versions they produce.
- The target names a **profile**, not a bare version: `{"type": "rpak", "profile": "apex-s20"}`. A profile (table in `respak-respawn`) fixes the pak version and each kind's version/variant. If any item can't be produced, the output fails with `UnsupportedVersion` **before writing**.
- Writers stream large data to the output operator. No core changes are needed to add them.

## 6. I/O: every file is a path on an OpenDAL operator
- **Inputs may be remote and expensive.** A host may plug in a backend that downloads byte ranges on demand (e.g. Steam depots, host code only). A 60 GB starpak must cost its table of contents plus the requested few MB, never a full download:
  - sizes come from `stat`, never from reading;
  - every read is ranged; a whole-file read only where the format requires it (a compressed `.rpak` body), documented in its doc comment;
  - no read-ahead beyond the range plus a small bounded margin; nearby small reads may be coalesced;
  - small, expensive-to-re-read data (a starpak's table of contents) goes to the plugin cache;
  - tested with a byte-counting `Operator` layer on a synthetic multi-GB starpak.
- **Containers are OpenDAL services.** A container (rpak, starpak, later VPK, mbnk) exposes its contents as a read-only `Operator`, its **volume**. Readers always work on (`Operator`, path), at any nesting depth, and OpenDAL layers (byte counting, logging, chunk cache) work inside containers too.
- **Small per-container code:** core provides one generic adapter implementing OpenDAL's service traits over a small read-only trait (`stat`, ranged `read`, `list`).
- **Virtual paths** are resolved left to right: the longest prefix that is a real file is scored; if it is a container, the rest is resolved in its volume; repeat. Real files win over container entries. Globs are supported.
- **Container compression is the container's business;** its volume decompresses on demand and documents the cost:
  - chunked with an index: only the chunks covering the range;
  - one stream (`.rpak` body; flags RTech `1<<8`, Oodle `1<<9`, zstd `1<<15`): decompress forward, keep tables and a bounded set of pages, restart only for an evicted page. Costs the `.rpak`'s size, never its starpaks'.
- **Compression inside a format** (texture mips, model LODs) stays in that format: ranged read of the unit, then `compressed → blob`. Units are small, so cost is bounded.
- **Codecs are shared mediator edges** (RTech, Snowflake, Oodle in respawn; zstd where first needed), so a host can also supply a licensed codec (Oodle).
- **rpak volume (proposal):**
  ```
  common.rpak/
    texture/foo.txtr          asset header; stat carries kind, version, header size, guid
    0x8A3F12C4D5E6F701.txtr   the same asset by GUID (names may be unknown)
    texture/foo.txtr.stream   streamed data → mandatory starpak
    texture/foo.txtr.opt      streamed data → optional starpak
    .pages/12                 one decompressed page (target of rpak.ptr)
  ```
  The extension is the asset's kind; a name is hashed to its GUID, so named paths work without stored names. Starpaks are volumes too (`x.starpak/<offset>`, sized by their table of contents) and are opened lazily.
- **Companion files** (starpaks, patches, `common.rpak`) are opened only when the current request needs them. Missing → `MissingDependency{item, file}`, except a missing optional starpak when the handler can fall back to lower mips.

## 7. Memory and caching
- **Now:** values live only while in flight (dedup), plus mounted volumes for the `Engine` call.
- **Later:** an in-memory cache as an optional Dedup retention policy (size-bounded LRU weighted by `approx_size`), off by default.
- **Plugin cache:** optional `Operator`; each plugin gets `cx.cache() -> Option<PluginCache>` namespaced by plugin id + schema version (`get`, `put`, `remove`, `get_json`, `put_json`). Core never interprets contents (e.g. respawn: GUID → name, GUID → containing file). Plugins must work without it; cache errors become `Warning` events.

## 8. Public API (`Engine`)
- `Engine::builder().input(op).output(op).cache(op?).plugin(p).middleware(m).max_concurrency(n).event_capacity(n).build()`; `build()` fails on ambiguous paths.
- `discover(path, &DiscoverOptions{depth}, &cancel) -> Result<Discovery>` · `convert(&ConvertRequest, &cancel) -> ConvertReport` · `open(path_or_glob, &cancel) -> Result<Vec<Value>>` · `events()` · `formats() -> FormatGraph`
```json
{ "version": 1,
  "outputs": [
    { "from": "paks/Win64/common.rpak/texture/foo.txtr", "to": { "type": "png", "mips": "all" } },
    { "from": "paks/Win64/common.rpak/0x8A3F12C4D5E6F701.rmdl", "to": { "type": "cast" }, "path": "custom/w_r97.cast" },
    { "from": "paks/Win64/common.rpak/**/*.txtr", "to": { "type": "dds" } },
    { "from": [ "art/icon.png", { "path": "mods/thing.bin", "format": "txtr" } ],
      "to": { "type": "rpak", "profile": "apex-s20" }, "path": "out/custom.rpak" } ] }
```
- `from`: a path, glob or list; `{path, format}` overrides detection. Several files → one bundle if the target accepts `bundle`, otherwise one conversion each.
- `to.type` is the target format; other fields are its options, plus optional `via`.
- `path` defaults to the path inside the innermost container with the target's extension (`texture/foo.png`). Existing outputs are overwritten.
- Outputs run in parallel up to `max_concurrency`; one failure doesn't stop the others.
- `ConvertReport { outputs: [ Ok{files, dependencies} | Err{error} ] }`
- `Discovery { path, entries: [ { path, format, version, score, alternatives, name?, targets: [ {type, label, defaults} ] } ] }`. Targets are formats reachable in the graph (no decoding); an entry's `path` goes straight into `from`.

## 9. Errors
`Error(Arc<ErrorKind>)`, serialized as kind + message: `UnknownFormat`, `AmbiguousFormat{candidates}`, `UnsupportedVersion{format, version}`, `NoPath{from, to}`, `NoMatch`, `InvalidOptions`, `NotFound`, `Storage(opendal::Error)`, `MissingDependency{item, file}`, `DependencyCycle{chain}`, `Corrupt{context}`, `Cancelled`, `Custom{source, message}`.

## 10. CLI (`respak-cli`)
- `lib.rs`: `async fn run(cli, engine, out) -> Result<ExitCode>`, testable in-process with `services::Memory`.
- `main.rs`: `services::Fs` operators from `--root`, `--out`, optional `--cache`; registers plugins; events to stderr; Ctrl-C → cancel.
- `formats` · `discover <path>... [--depth N]` · `run <request.json | ->` · `convert <path|glob> --to '<json>' [--path P]`. Non-zero exit if any output failed.

## 11. Open questions (decide with the user)
- Raw-byte / chunk caching of the input operator (the user asked to revisit; matters for remote inputs).
- In-memory cache design (§7).
- OpenDAL pin: API names; size of the generic service adapter; can `Metadata` carry custom fields (kind, version, header size); is a host ranged-read backend (e.g. Steam depot) practical?
- rpak volume naming (§6 proposal).
- Exact capability methods; option schemas for targets (`schemars`?).
- FFI shape for the async API ("JSON in/out + event callback").
- zstd-compressed paks (flagged, unsupported by rsx). Codec licensing (Oodle).

## 12. rsx reference
- **Avoid rsx's problems:** global mutable settings; dependencies exported several times concurrently to one path; exporters writing straight to disk with repeated path logic; failures as bare `bool`.
- **Where to look** (paths under `src/game/rtech/` unless noted):
  - `cpakfile.{h,cpp}`: pak headers v6/v7/v8, asset entries (`headerStructSize`), patches, compression flags. Starpak offsets: low 12 bits = starpak index, rest = 4K-aligned offset (`cpakfile.h:1175`); starpak table of contents at file end (`cpakfile.cpp:635`).
  - `starpak_source.h`: starpak range reads. Compressed streamed data: per-mip codec bits `assets/texture.h:1072`, per-LOD `src/core/mdl/modeldata.h:340`, codec enum `utils/utils.h:148`.
  - `assets/*.cpp`: per-kind parsing. Variant detection: `material.cpp`, `model.cpp` `GetModelVersionFromAsset`, `animseq.h`, `settings.cpp`, `shaderset.cpp`, `ui_rtk.cpp`; `AssetVersion_t` in `src/game/asset.h`.
  - `src/core/mdl/`: Cast/SMD/QC writers · `src/core/cache/cachedb.h`: GUID → name/file database.
