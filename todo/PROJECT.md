# respak — project summary

respak is a Rust library, plus a separate CLI, that reads files from Respawn games and converts their contents into usable formats (e.g. an `.rpak` texture to PNG/DDS, a model to Cast), and later back into Respawn formats. Its architecture is designed from scratch. Format code (pak loading, codecs, asset parsing) is **ported from the C++ tool rsx** (`C:\Users\chiririll\Projects\rsx`) and fitted into that architecture; rsx's own architecture is not carried over (§12). Version coverage = whatever rsx supports.

**Status:** foundation done (workspace, errors, values, capabilities); slices 05 (PNG ↔ DDS), 06 (`formats`, `discover`), 07 (rpak listing), 08 (rpak texture → DDS) 09 (BC decoding, `TypedTarget` options), 10 (streamed mips, starpaks) and 11 (batch requests, globs, composite outputs) done; now building vertical slices. The task list is in [`tasks/`](tasks/README.md).

---

## 1. Workflow and rules
This document is the **target design**. It is built in **vertical slices**: thin end-to-end paths (plugin → core → engine → CLI), each ending in a command the user can run. A part of the design is built only when a slice needs it. Per slice ([details](tasks/README.md)):
1. **Sketch** the interfaces this slice needs (compiles, `todo!()` bodies) and propose test cases.
2. **The user reviews the sketch and agrees the tests.** Never invent the test suite alone.
3. **Implement test-first (TDD).**
4. **The user runs the demo.** What was learned updates this document before the next slice.

If a slice shows that part of this design doesn't work, stop and tell the user. Don't work around it.

Rules:
- **Prefer existing, well-maintained crates.** The one agreed exception is the mediator, hand-written on purpose.
- **Ask the user when something is unclear.** Don't guess.
- **No Steam or downloader code.** Where bytes come from is the host's business.
- **Read only the byte ranges you need** (§6).
- **A format handler only handles its own format.** For anything else it asks the mediator: "here is a value, convert it to format X".
- **Traits and data types:** a format may implement core traits (capabilities, §5.4) and traits of its own crate. Conversions to formats of *other* crates go only through core traits. Inside one crate, modules may share data types of each other's formats (e.g. `txtr` reads `rpak.asset`), never call each other's handlers.
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
| Storage (input, output, cache) | `opendal` | Pre-1.0: **pinned `=0.59.4`** (latest stable, checked in task 01), `default-features = false`, features `services-fs`, `executors-tokio`. `services::Fs` for disk, `services::Memory` for tests (always available, lives in `opendal-core`). API notes below. |
| Async | `tokio`, `tokio-util` (`CancellationToken`), `async-trait`, `futures` (`Shared`) | tokio is a direct dependency of core |
| Data | `serde`, `serde_json`, `bytes` | requests, targets, reports, discovery are JSON |
| Errors / logs | `thiserror`, `tracing` | |
| CLI | `clap` (derive), `anyhow` | CLI crate only |
| Candidates | `globset` (path globs), `moka` (in-memory cache, later) | added when needed |

Rejected, don't reopen without asking: a home-made filesystem replacing OpenDAL; `object_store`; `vfs`; `tower` for the mediator; a message broker; first-match `can_handle` routing (→ scoring); shared intermediate structs such as one `Texture` for every format (→ capabilities); a declarative probe DSL (→ score functions, §5.3); a path table precomputed at build time (→ per-request path search, §5.2); format-specific fields in OpenDAL `Metadata` (→ typed container entries, §5.4).

## 4. Overview
Everything is a **value tagged with a format**. Handlers are **edges** between formats. The mediator detects formats with **score functions** and converts by finding the **cheapest path** through the graph. Files are addressed by **virtual paths** that may go into containers; every container exposes an **OpenDAL operator** (its volume) and hands out **typed entries**.
```
paks/Win64/common.rpak/texture/foo.txtr  →  png

common.rpak is a file → scored as rpak → mounted → entry("texture/foo.txtr") = rpak.asset{kind txtr, version 10, …}
rpak.asset ─read─▶ txtr ─(Bitmap)─▶ png ─▶ output
   header           read(texture/foo.txtr)             → served by the rpak volume, pointers relocated
   pointed-to data  read(.mem, range)                  → decompressed pages, served by the rpak volume
   streamed mips    read(texture/foo.txtr.stream, range) → mapped onto x.starpak → ranged read on the host
   compressed mip   convert(compressed{oodle,…} → blob)  → codec handler
```
`txtr` reads the `rpak.asset` data type (same crate) but never parses rpak structures; `rpak` never learns what `txtr` is.

## 5. Core architecture

### 5.1 Values and formats
```rust
pub type Value = Arc<dyn Data>;

pub trait Data: Any + Send + Sync + Debug {
    fn format(&self) -> &FormatId;                  // name + detected version, e.g. txtr@10.0
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
- Formats are names owned by the crate that defines them. Two plugins defining the same format name → build error. Any plugin may add edges to or from any format name.
- Core formats (an encoder returns a `blob` **tagged with its own format**, e.g. `BlobData::with_format(png, ..)`; core writes any value with `as_blob`):
  - `file { op: Operator, path, range: Option<Range<u64>> }`: on the host operator or inside a container volume
  - `blob`: bytes in memory
  - `bundle`: several values (one output with several sources)
  - `compressed { codec, data, decoded_len }`: decompressed to `blob` by a plugin's codec handler
- Plugin formats, including internal plumbing: `rpak`, `starpak`, `rpak.asset`, `rpak.guid`, `txtr`, … (respawn); `png`, `dds`, … (formats).
- **Keys:** a file's key is its full virtual path (+ range); a converted value's key is source key + target format + normalized options (defaults filled in, fields sorted).

### 5.2 Edges, registration, paths
- **Read edges** turn a generic value into a specific one (`file → rpak`, `rpak.asset → txtr`). Each has a score function (§5.3).
- **Convert edges** turn one format into another (`compressed → blob`, `rpak.guid → rpak.asset`, `Bitmap → png`). Each has a cost (1 = lossless, higher = lossy) and starts from a format or a capability.
- Registration is grouped per format. Each format **declares its capabilities** (`provides`), so paths can be planned and discovery can list targets without decoding:
```rust
impl Plugin for RespawnPlugin {
    fn id(&self) -> &str { "respawn" }
    fn register(&self, r: &mut Registry) {
        r.format("rpak")      .from("file", RpakOpen).extensions(["rpak"]).provides::<dyn Container>();
        r.format("starpak")   .from("file", StarpakOpen).extensions(["starpak"]).provides::<dyn Container>();
        r.format("rpak.asset");                                  // handed out by the rpak volume (§5.4)
        r.format("rpak.guid") .to("rpak.asset", RpakLocate);     // dependency → asset (§6)
        r.format("txtr")
            .from("rpak.asset", TxtrRead).extensions(["txtr"])
            .provides::<dyn Bitmap>().provides::<dyn Metadata>();
    }
}
// respak-formats
r.format("png").from("file", PngRead).extensions(["png"]).provides::<dyn Bitmap>()
               .from_cap::<dyn Bitmap>(PngEncode).cost(2);
r.format("dds").from("file", DdsRead).extensions(["dds"]).provides::<dyn Bitmap>()
               .from_cap::<dyn Bitmap>(DdsEncode);       // BC blocks copied as-is, cost 1
```
- **Path selection** happens per request, from the source value's format and declared capabilities; nothing is precomputed at build time. Cheapest path wins. Ties are broken deterministically: direct edge before capability edge, then fewer hops, then plugin registration order; the chosen path is logged as a `Warning` event. A request may pin a route with `via`: its formats must appear in the path in that order (the source and target count). The planner (slice 06) is a search over direct (`.to`) and capability edges; order of preference: cost, then fewer capability steps, then fewer hops, then registration order. The path is logged only when more than one complete path existed.

### 5.3 Detection by scoring
- Each read edge has a **score function**: it looks at the input and returns `None` (not this format) or `Some(Detected { score, version })`. The version includes the layout variant (`Version.minor`), so `identify` returns the full version and handlers don't detect it again.
```rust
#[async_trait]
pub trait Detect: Send + Sync {
    async fn score(&self, input: &Probe<'_>, cx: &Cx) -> Result<Option<Detected>>;
}
pub struct Detected { pub score: u32, pub version: Option<Version> }
```
- `Probe` gives the source value, its extension and length (from `stat`), and a **header prefix**: the mediator reads it once, sized to the largest `header_len()` among the candidates, and shares it. A score function may make further ranged reads if it really needs to (e.g. sampling data to tell variants apart).
- Core ships helpers for score functions: `magic`, `u16le/u32le/…`, `within`, etc. They are plain functions, not a DSL.
- **Order:**
  1. Candidates whose registered extensions match the input's extension are scored first.
  2. If none of them returns `Some`, every read edge from the source format is scored.
  3. Highest score wins. A real tie → `AmbiguousFormat`; the request can set `format`.
- **Score convention** (documented, so plugins stay comparable): a matched magic or container-provided kind ≈ 10, each further confirming field ≈ 1–5, extension alone ≈ 1.
- **Variants:** one handler per format, branching on `Version.minor` (e.g. rsx: `matl` 23.1/23.2 by header size, `rmdl` 12.x by `studiohdr` offsets). A separate handler only for big changes, distinguished by its score function. Known format but unknown variant → `UnsupportedVersion`, not `UnknownFormat`.
- Discovery reports winner, score and runners-up.

### 5.4 Capabilities
Each format keeps its full native type (`TxtrAsset`, …) and implements the capability traits that apply. Capabilities are lazy views.
- `Blob`: in-memory bytes · `Metadata`: name, guid, dependencies
- `Container`: `volume() -> Operator` and `entry(path) -> Result<Value>`. The default `entry` returns `file { volume, path }`; a container overrides it to hand out typed entries carrying data that isn't in the entry's bytes. Example: the rpak volume returns `rpak.asset { kind, version, header_size, guid, header: file, mem: file, stream: Option<file>, opt: Option<file> }`, because an asset's version and header size live in the pak's asset entry, not in the asset bytes.
- `Bitmap`: dimensions, depth, array/faces, mip count, pixel format, `read_mip(level, layer)`
- **Registration names capabilities** with `Cap` (`provides::<dyn Bitmap>()`); it carries only a name. Handlers read a capability from the value (`value.as_bitmap()`); a `Cap::of(&dyn Data)` accessor is not possible with the current lifetimes. (Slice 05.)
- `Mesh`, `Skeleton`, `Animation`, `Table`, `Audio`: skeletal, refined with the first real converters
- Core ships owned helpers (e.g. `BitmapBuf`) for converters that create data. Plugins may add capabilities via `Data::capability`.

### 5.5 Mediator, context, linking
```rust
impl Mediator {
    pub async fn convert(&self, source: Value, to: Target, cx: &Cx) -> Result<Value>;  // Target = { format, options: serde_json::Value }
    pub async fn identify(&self, source: Value, cx: &Cx) -> Result<Value>;             // read edges only
    pub fn publish(&self, e: Event);
    pub fn subscribe(&self) -> broadcast::Receiver<Event>;
}
```
- **One request type** (`Convert`). Open, select, locate, decode and encode are paths, values and edges; plugin-internal needs are private formats, not private requests. `identify` goes through the same middleware.
- **Linking:** a converter asks for what it needs (a Cast encoder converts its textures to PNG and writes them via `cx.write`). There is no `with_dependencies` flag.
- **Output:**
  - Core writes any `Blob` value to the output operator. Multi-file or streaming writers use `cx.write` directly.
  - **A composite output (more than one file: a model with textures, a texture with `mips: "all"`) gets its own folder**: `w_r97/w_r97.cast` + `w_r97/textures/*.png`. Two composites sharing a texture each write their own copy.
  - **Written paths are tracked for the whole `Engine` call** (output path → value key): a second write of the same key to the same path is skipped; a different key on an already written path → `OutputConflict`.
- **`Cx`:** mediator handle, call chain of keys, `CancellationToken`, input/output operators, `cache()`, `write(path, value)`, `blocking(..)` (`spawn_blocking`, bounded by `blocking_limit`) and `io(..)` (runs an I/O-bound future, bounded by `io_limit`).
- **`TypedTarget<T>`** parses target options into a plugin struct (missing fields from `Default`); typed edge adapters downcast `Value` to the handler's type.

### 5.6 Middleware
`async fn around(&self, req: &Convert, cx: &Cx, next: Next<'_>) -> Result<Value>`. Built-ins, in order:
1. **Cancellation:** `Cancelled` if the token is triggered at request start. Long-running handlers check the token themselves.
2. **Cycle guard:** a key already in the call chain → `DependencyCycle`. Also a **wait-for graph** over in-flight keys (recorded when a request joins an in-flight future), so concurrent chains A→B / B→A can't deadlock.
3. **Dedup:** merges **concurrent** requests: in-flight map (source key, target) → `Shared` future. Removed on completion, except mounted volumes, which live until the `Engine` call ends. Non-overlapping requests for the same thing are converted again until the in-memory cache exists (§7); the written-paths map (§5.5) keeps that from writing a file twice.
4. **Tracing** span per request. 5. **Events:** started / finished / failed.

- **No per-request concurrency limit** (a parent awaiting children while holding a permit deadlocks). Limits apply only to top-level outputs (`max_concurrency`) and to `cx.blocking` / `cx.io`.
- Results are shared, so `Value` is `Arc` and errors are `Arc`-wrapped (`opendal::Error` isn't `Clone`). Hosts can add middleware.

### 5.7 Events
- `#[non_exhaustive] enum Event`: `RequestStarted`, `RequestFinished`, `RequestFailed`, `InputOpened{path, entries}` (a container was mounted; `entries` = number of entries), `OutputWritten{path}`, `Progress{task, done, total}`, `Warning{message}`, `Custom{source, name, data}`.
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
- **Containers are OpenDAL services.** A container (rpak, starpak, later VPK, mbnk) exposes its contents as a read-only `Operator`, its **volume**, and resolves paths to typed entries (§5.4). Readers always work on (`Operator`, path), at any nesting depth, and OpenDAL layers (byte counting, logging, chunk cache) work inside containers too.
- **Small per-container code:** core provides one generic adapter implementing OpenDAL's service traits over a small read-only trait `VolumeSource` (`stat`, ranged `read`, `list`).
- **Virtual paths** are resolved left to right: the longest prefix that is a real file is scored; if it is a container, the rest is resolved with its `entry(path)`; repeat. Real files win over container entries.
- **Globs:** a glob descends into a container only where a pattern segment matches the container file and more segments follow (`paks/Win64/*.rpak/**/*.txtr`). `**` alone never enters containers (`paks/**/*.txtr` doesn't open any rpak). No match → `NoMatch`.
- **Container compression is the container's business;** its volume decompresses on demand and documents the cost:
  - chunked with an index: only the chunks covering the range;
  - one stream (`.rpak` body; flags RTech `1<<8`, Oodle `1<<9`, zstd `1<<15`): decompress forward, keep tables and a bounded set of pages, restart only for an evicted page. Costs the `.rpak`'s size, never its starpaks'.
- **Compression inside a format** (texture mips, model LODs) stays in that format: ranged read of the unit, then `compressed → blob`. Units are small, so cost is bounded.
- **Codecs are shared mediator edges** (RTech, Snowflake, Oodle in respawn; zstd where first needed), so a host can also supply a licensed codec (Oodle).
- **rpak volume (proposal):**
  ```
  common.rpak/
    texture/foo.txtr          asset header, pointers relocated (below)
    0x8A3F12C4D5E6F701.txtr   the same asset by GUID (resolves, but isn't listed when the name is known)
    texture/foo.txtr.stream   streamed data → mandatory starpak
    texture/foo.txtr.opt      streamed data → optional starpak
    .mem                      all decompressed pages as one flat virtual file
  ```
  - The extension is the asset's kind; a name is hashed to its GUID, so named paths work without stored names. Listing shows each asset once: by name if known, otherwise by GUID.
  - **The rpak volume serves only the bytes a handler needs.** It uses the pak's pointer relocation tables (no knowledge of asset kinds) to rewrite pointers in served bytes into offsets in `.mem`; a handler follows a pointer with a ranged read on `.mem`. Asset data pointed to by the asset entry (not the header) is reached the same way.
  - Starpaks are volumes too (`x.starpak/<offset>`, sized by their table of contents) and are opened lazily.
- **Companion files** (starpaks, `common.rpak`) are opened only when the current request needs them. Missing → `MissingDependency{item, file}`, except a missing optional starpak when the handler can fall back to lower mips.
- **Locating a GUID dependency** (`rpak.guid → rpak.asset`, e.g. a material in a level pak referencing a texture in `common.rpak`), in order:
  1. the same pak;
  2. `common.rpak` in the same directory;
  3. the respawn plugin's search list (`RespawnPlugin::new().search(["paks/Win64/*.rpak"])`, CLI `--search`);
  4. the GUID → file index in the plugin cache, if present;
  5. otherwise `MissingDependency`.

  It never scans every pak under the input root on its own.
- **Loose assets** (an asset outside any rpak, e.g. `mods/thing.bin`) have no asset entry, so the request must give the full format: `{ "path": "mods/thing.bin", "format": "txtr@10" }`.

## 7. Memory and caching
- **Now:** values live only while in flight (dedup), plus mounted volumes for the `Engine` call.
- **Later:** an in-memory cache as an optional Dedup retention policy (size-bounded LRU weighted by `approx_size`), off by default.
- **Plugin cache:** optional `Operator`; each plugin gets `cx.cache() -> Option<PluginCache>` namespaced by plugin id + `Plugin::cache_version()` (default 0; bump when the cached schema changes). Methods: `get`, `put`, `remove`, `get_json`, `put_json`. Core never interprets contents (e.g. respawn: GUID → name, GUID → containing file). Plugins must work without it; cache errors become `Warning` events.

## 8. Public API (`Engine`)
- `Engine::builder().input(op).output(op).cache(op?).plugin(p).middleware(m).max_concurrency(n).blocking_limit(n).io_limit(n).event_capacity(n).build()`; `build()` fails on duplicate format definitions.
- `discover(path, &DiscoverOptions{depth}, &cancel) -> Result<Discovery>` (`depth` = how many container levels to open) · `convert(&ConvertRequest, &cancel) -> Result<ConvertReport>` (`Err` only for an invalid request, e.g. unknown `version`) · `open(path_or_glob, &cancel) -> Result<Vec<Value>>` · `events()` · `formats() -> FormatGraph`
```json
{ "version": 1,
  "outputs": [
    { "from": "paks/Win64/common.rpak/texture/foo.txtr", "to": { "type": "png", "mips": "all" } },
    { "from": "paks/Win64/common.rpak/0x8A3F12C4D5E6F701.rmdl", "to": { "type": "cast" }, "path": "custom/w_r97" },
    { "from": "paks/Win64/common.rpak/**/*.txtr", "to": { "type": "dds" } },
    { "from": [ "art/icon.png", { "path": "mods/thing.bin", "format": "txtr@10" } ],
      "to": { "type": "rpak", "profile": "apex-s20" }, "path": "out/custom.rpak" } ] }
```
- `from`: a path, glob or list; `{path, format}` overrides detection. Several sources (a glob or a list) → one bundle if the target format has an edge from `bundle`, otherwise one conversion each. This doesn't depend on how many files the glob matched.
- `to.type` is the target format; other fields are its options, plus optional `via`.
- `path` defaults to the path inside the innermost container with the target's extension (`texture/foo.png`), or the folder for a composite output (`texture/foo/`). For a source outside any container it's the path relative to the input root. Existing outputs are overwritten; two outputs of one call landing on the same path → `OutputConflict` (§5.5).
- Outputs run in parallel up to `max_concurrency`; one failure doesn't stop the others.
- `ConvertReport { outputs: [ { from, result: Ok{files, dependencies} | Err{error} } ] }`: one entry per conversion (a glob expanding to N conversions gives N entries). `files` = paths written; `dependencies` = linked values it needed (key + format).
- `Discovery { path, entries: [ { path, format, version, score, alternatives, name?, targets: [ {type, label, defaults} ] } ] }`. Targets are formats reachable from the entry's format and declared capabilities (no decoding); an entry's `path` goes straight into `from`.

## 9. Errors
`Error(Arc<ErrorKind>)`, `#[non_exhaustive]`, serialized as kind + message:
- `UnknownFormat`: nothing scored the input.
- `AmbiguousFormat{candidates}`: equal top scores.
- `UnsupportedVersion{format, version}`: known format, unknown version or variant.
- `NoPath{from, to}`: no route in the graph.
- `NoMatch`: a glob matched nothing.
- `NotFound`: a concrete path doesn't exist.
- `InvalidRequest`: malformed request JSON or unknown request `version`.
- `InvalidOptions`: target options don't parse.
- `OutputConflict{path}`: two different values written to one path.
- `Storage(opendal::Error)`, `MissingDependency{item, file}`, `DependencyCycle{chain}`, `Corrupt{context}`, `Cancelled`, `Custom{source, message}`.

## 10. CLI (`respak-cli`)
- `lib.rs`: `async fn run(cli, engine, out) -> Result<ExitCode>`, testable in-process with `services::Memory`.
- `main.rs`: `services::Fs` operators from `--root`, `--out`, optional `--cache`; `--search <glob>...` for GUID lookup; registers plugins; events to stderr; Ctrl-C → cancel.
- `formats` · `discover <path>... [--depth N]` · `run <request.json | ->` · `convert <path|glob> --to '<json>' [--path P]`. Non-zero exit if any output failed.

## 11. Open questions (decide with the user)
- ~~License~~ decided (the user delegated all remaining decisions): respak is **AGPL-3.0-only**, because it ports AGPL-3.0 rsx code. `LICENSE` and the workspace `license` field say so. Reversible only if the rsx authors grant otherwise.
- ~~First milestone~~ decided: port rsx, covering every game and pak version it supports; first formats txtr → png/dds.
- **rpak patches** (`common(01).rpak`, …, `patch_master.rpak`): out of scope for v1. Later the base pak's volume probably applies the latest patch transparently.
- Raw-byte / chunk caching of the input operator (the user asked to revisit; matters for remote inputs).
- In-memory cache design (§7).
- Slice 07 findings: the `VolumeSource` -> `Operator` adapter is ~150 lines (`Operator::from_parts(OperationContext::default(), Arc::new(service))`; a lister, a `StreamRead` reader, rest `()`). OpenDAL rejects an explicit range past the end (`RangeNotSatisfied`), so callers clamp. A compressed rpak is one RTech stream: it is decompressed whole into memory on first use (backlog: bounded streaming decoder).
- OpenDAL pin (`=0.59.4`, API names verified from source in task 01). Still open: size of the generic service adapter; is a host ranged-read backend (e.g. Steam depot) practical? Verified API, and where it differs from older docs:
  - Present: `Operator`, `stat`, `exists`, `read`, `read_with(p).range(..)`, `reader`/`reader_with` (`Reader::read(range)`, `fetch(Vec<Range>)`), `list`/`lister`, `write` (returns `Metadata`, not `()`), `services::Fs` (feature `services-fs`, needs tokio), `services::Memory` (always available).
  - `opendal` is a facade over `opendal-core` and per-service crates; custom-service API is under `opendal::raw`.
  - `Access`/`Accessor` → `raw::Service` (`ServiceInfo`, dyn `ServiceDyn`, `Servicer = Arc<dyn ServiceDyn>`). Methods take `&OperationContext` first; `read`/`write`/`list`/`delete` are sync and return the reader/lister, `stat`/`create_dir` are async.
  - Read-only service (slice 07): implement `raw::Service` with `Reader = oio::StreamReader<R>` where `R: oio::StreamRead` (one method, `open(range)`), `Lister: oio::List` (`next() -> Option<Entry>`), the rest `()`; report only `stat`/`read`/`list` in `capability()` (use `..Default::default()`); return `ErrorKind::Unsupported` otherwise. Template: `MemoryBackend` in `opendal-core`. Build with `Operator::new(Builder)` (needs `Builder` + `Configurator`) or `Operator::from_inner(Arc<dyn ServiceDyn>)`.
  - `Layer` has a new shape (`apply_service`, `apply_context`); `Operator::new` has no `.finish()`; `Scheme` enum is replaced by string schemes; option structs grow fields, never use exhaustive literals.
  - Not verified: running with `default-features = false` and no executor feature; we enable `executors-tokio`.
- rpak volume layout (§6 proposal): built in slices 07 and 08: files `0x<GUID>.<kind>` (a name is hashed to its GUID), headers with pointers relocated to `.mem` offsets (u64, all ones = null), and a hidden `.mem`. Pages of unpatched paks are concatenated, so `.mem` offset = `page_base[page] + offset`.
- Exact capability methods; option schemas for targets (`schemars`?).
- FFI shape for the async API ("JSON in/out + event callback").
- zstd-compressed paks (flagged, unsupported by rsx). Codec licensing (Oodle).

## 12. Porting from rsx
**rsx code is reviewed before it is ported, never translated line by line.** In every slice that ports code, the sketch step includes an **rsx review**, written into the task under `## rsx review`. The user approves it at the gate, before any porting starts:
1. **What it does:** the rsx files and functions involved; the layouts, constants and version/variant branches they encode.
2. **Problems:** bugs, duplicated or dead branches, unchecked offsets and sizes, whole-file reads, global state, failures as `bool`, and anything that breaks the I/O rules (§6).
3. **Fit:** where each piece goes in respak (score function, volume, typed entry, capability, codec edge) and what changes to make it fit. Examples: ranged reads instead of loading the file; `Result` with the §9 errors instead of `bool`; one handler branching on `Version` instead of a copy per version; tables instead of `switch` chains.

Rules:
- **Keep** the format knowledge: struct layouts, constants, version and variant detection, codec algorithms. Each ported module names its rsx source files in its doc comment.
- **Don't keep** rsx's architecture or code structure where a better fit exists. Ported logic goes into score functions, volumes, typed entries and capabilities.
- A behaviour change against rsx (a bug fix, a dropped branch) is listed in the review, so the demo comparison with rsx can account for it.
- Use subagents for the source sweep.
- **Avoid rsx's problems:** global mutable settings; dependencies exported several times concurrently to one path; exporters writing straight to disk with repeated path logic; failures as bare `bool`.
- **Where to look** (paths under `src/game/rtech/` unless noted):
  - `cpakfile.{h,cpp}`: pak headers v6/v7/v8, asset entries (`headerStructSize`, version), pointer relocation, patches, compression flags. Starpak offsets: low 12 bits = starpak index, rest = 4K-aligned offset (`cpakfile.h:1175`); starpak table of contents at file end (`cpakfile.cpp:635`).
  - `starpak_source.h`: starpak range reads. Compressed streamed data: per-mip codec bits `assets/texture.h:1072`, per-LOD `src/core/mdl/modeldata.h:340`, codec enum `utils/utils.h:148`.
  - `assets/*.cpp`: per-kind parsing. Variant detection: `material.cpp`, `model.cpp` `GetModelVersionFromAsset`, `animseq.h`, `settings.cpp`, `shaderset.cpp`, `ui_rtk.cpp`; `AssetVersion_t` in `src/game/asset.h`.
  - `src/core/mdl/`: Cast/SMD/QC writers · `src/core/cache/cachedb.h`: GUID → name/file database.
