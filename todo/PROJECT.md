# respak — project summary

respak is a Rust library, plus a separate CLI, that reads files from Respawn games and converts what is inside them into usable formats. For example, it can turn a texture in an `.rpak` into a PNG or DDS, or a model into Cast or SMD. It is designed from scratch. The C++ tool **rsx** (`C:\Users\chiririll\Projects\rsx`) is a source of format knowledge only, not code to port.

**Status:** the design is agreed and nothing is implemented. The tasks are in [`tasks/`](tasks/README.md).

---

## 1. Workflow (mandatory)
1. **Interfaces first.** Write the types, traits, crate layout and doc comments, plus skeleton plugins and the CLI. It must compile, logic bodies are `todo!()`, and there are no tests yet.
2. **The user reviews the interfaces.**
3. **Test cases are defined together with the user.** Do not invent the test suite alone.
4. **Implementation, test-first (TDD).**

Rules for anyone working on this project:
- **Prefer existing, well-maintained crates over hand-written infrastructure.** The user doesn't want reinvented wheels. The one agreed exception is the mediator/dispatcher, which is hand-written on purpose.
- **Ask the user when something is unclear.** Don't guess. The user explicitly prefers clarifying up front.
- **No Steam or downloader code in this project.** respak is a converter. Where bytes come from is the host's business, handled through the filesystem abstraction.
- **Never load a large file fully into memory unless it's necessary.** Reading a whole file is fine when the format requires it, for example a pak's full asset table to list its entries. Pulling a multi-GB starpak into memory to get one texture is not. Read only what the current request needs, with range reads.

---

## 2. Goals
- Convert any supported input file into requested outputs.
- Be **abstract over input formats** (RPak, Miles banks, Source MDL/VPK, future formats and versions) and **over output formats** (PNG, DDS, Cast, SMD, …). The core contains no format knowledge.
- Support **all versions of all formats, including future ones**, by registering handlers per kind and version range. An unknown version gives an explicit `UnsupportedVersion` error.
- It is a **Rust crate first**. FFI to other languages comes later, so keep the public API wrappable: JSON in, JSON out, and object-safe traits.
- A **CLI** for manual use and testing lives in its own crate.

## 3. Crate layout (Cargo workspace)
```
respak/
  crates/
    respak-core/    # mediator, middleware, events, core request types, intermediate types,
                    # plugin cache view, Engine (public API), errors. NO format code.
    respak-cli/     # host binary: chooses plugin crates, registers them, CLI commands
    respak-rpak/    # RPak container + every asset kind inside it (txtr, matl, mdl_, uiia, aseq, dtbl, …)
    respak-image/   # PNG / DDS encoders for core::types::Texture
  # future: respak-miles (mbnk/mstr), respak-source (MDL v52/53, VPK), respak-cast, respak-smd, …
```
- There is **one crate per format family.** Everything belonging to the RPak world lives in `respak-rpak`, and Miles files get their own crate.
- **Plugin crates depend only on `respak-core`, never on each other.** They communicate through the mediator and the shared intermediate types. For example, `rpak` decodes a texture into `core::types::Texture`, and `image` encodes any `core::types::Texture`. Neither crate knows the other exists.

```
            respak-core
   ▲        ▲        ▲        ▲
 rpak    miles    image    cast …        (each depends only on core)
   ▲                 ▲
   └──── respak-cli ─┘                   (host registers the plugins it wants)
```

## 4. Key dependencies
| Purpose | Crate | Note |
|---|---|---|
| Filesystem abstraction (input, output, cache) | **Apache OpenDAL** (`opendal`) | `Operator` plays the role of .NET `IFileSystem`. Use `services::Fs` for disk and `services::Memory` for tests; hosts can plug in any backend. It is pre-1.0, so **pin an exact version** and check the API names on docs.rs. |
| Async runtime | `tokio`, `tokio-util` (`CancellationToken`) | tokio is a direct dependency of core (an agreed decision) |
| Async traits | `async-trait` | needed so the traits can be used as trait objects (`dyn`) |
| Dedup of in-flight requests | `futures` (`future::Shared`) | |
| Serialization | `serde`, `serde_json` | requests, selectors, targets, reports and discovery are all JSON |
| Errors | `thiserror` | |
| Logging | `tracing` | |
| Byte buffers | `bytes` | |
| CLI | `clap` (derive), `anyhow` | CLI crate only |

Approaches that were rejected, so don't reopen them without asking:
- A **home-made filesystem trait.** Use OpenDAL instead.
- **`object_store`** as the filesystem layer: it requires write methods and its API churns.
- **`vfs`**: its async support is built on async-std and is being sunset.
- **`tower`** for the mediator: the user chose a hand-written dispatcher.
- A **real message broker**: not needed in-process.

## 5. Architecture: mediator
The core is an in-process **mediator**, like MediatR in .NET. Everything is a typed request routed to a plugin handler, through a middleware chain. Handlers can send further requests through the mediator.

```rust
pub trait Request: Send + Sync + 'static {
    type Response: Clone + Send + Sync + 'static;
    fn key(&self) -> RequestKey;               // identity for dedup + cycle detection
}
#[async_trait]
pub trait Handler<R: Request>: Send + Sync {
    fn info(&self) -> HandlerInfo;             // static metadata (see below)
    fn can_handle(&self, req: &R) -> bool;
    async fn handle(&self, req: R, cx: &Cx) -> Result<R::Response>;
}
pub trait Plugin { fn id(&self) -> &str; fn register(&self, m: &mut Mediator); }

impl Mediator {
    pub fn register<R: Request>(&mut self, h: Arc<dyn Handler<R>>);
    pub fn add_middleware(&mut self, m: Arc<dyn Middleware>);
    pub async fn send<R: Request>(&self, req: R, cx: &Cx) -> Result<R::Response>;
    pub fn publish(&self, e: Event);
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event>;
}
```
- **Routing:** the first registered handler whose `can_handle` returns true wins, in registration order (document this). If none matches, the result is `Error::NoHandler`.
- **`HandlerInfo` is static metadata,** so discovery works without decoding anything:
  - Decoders declare `kind`, `versions: VersionRange` and `produces` (the intermediate type name).
  - Encoders declare `target_type` (`"png"`) and `accepts` (intermediate type names).
  - The available targets for an item are computed from the registry: the item's kind and version select a decoder, its `produces` type is matched against what each encoder `accepts`, and the matching encoders are the targets.
  - When the kind is known but no version range matches, the error is `UnsupportedVersion`, not `NoHandler`.
- **`Plugin::register`** registers **all** of a crate's handlers. `RpakPlugin` registers:
  - the input handlers `OpenInput`, `Discover`, `Select`, `Locate` and `ReadItem`
  - one `Decode` handler per asset kind it understands, each with its version range
  - any private request types it uses internally
  
  Plugins may take options that enable a subset, e.g. `RpakPlugin::default().kinds(["txtr","matl"])`. Input plugins register no encoders.
- **Plugins may define their own request types.** For example, rpak could define a private `ReadStarpakRange`.
- **`Cx`** is the per-call context. It holds:
  - a mediator handle
  - the call chain of request keys
  - the `CancellationToken`
  - `cache() -> Option<PluginCache>`
  - the input and output `Operator`s
  - the helpers `blocking(..)` (CPU work via `spawn_blocking`) and `io(..)`

### Middleware
`trait Middleware { async fn around(&self, req: &dyn ErasedRequest, cx: &Cx, next: Next<'_>) -> Result<ErasedResponse>; }`. The built-in layers run in this order:
1. **Cancellation:** fail with `Cancelled` if the token has been triggered.
2. **Cycle guard:** a key already in the call chain gives `DependencyCycle`. The guard *also* keeps a **wait-for graph over in-flight keys**. Without it, two concurrent chains (A→B in one, B→A in the other) would each wait on the other's in-flight future forever.
3. **Dedup:** an in-flight map of `RequestKey` to `Shared` future. Identical concurrent requests share one result. **The entry is removed when the request completes.** The exception is `OpenInput`: an opened file's handle (header and tables) stays open for the rest of the `convert` call.
4. **Tracing:** a span per request.
5. **Events:** started, finished and failed events.

**There is no per-request concurrency limit.** A parent holds its permit while it awaits its children, so such a limit deadlocks. Limits apply only to top-level outputs in `Engine::convert` and to the work done in `cx.blocking`/`cx.io`.

Because results are shared between waiters, `Response: Clone` and errors are `Arc`-wrapped. `opendal::Error` isn't `Clone`.

### Events (pub/sub)
- `#[non_exhaustive] enum Event` has these variants:
  - `RequestStarted`, `RequestFinished`, `RequestFailed`
  - `InputOpened{path, files}`, `OutputWritten{path}`
  - `Progress{task, done, total}`
  - `Warning{message}`
  - `Custom{source, name, data: Value}`
- Events go through a `tokio::sync::broadcast` channel. A built-in `LogSubscriber` forwards them to `tracing`, and the CLI prints them to stderr.
- **Progress is published as events**, so there is no separate progress interface.
- A slow subscriber can miss events (`Lagged`), and that's acceptable. Results that matter are always in the returned report, never only in events.

## 6. Core request types
| Request | Response | Handled by |
|---|---|---|
| `OpenInput { path, format: Option<String> }` | `Arc<dyn OpenedInput>` | the input plugin; the right one is picked by `can_handle` (extension or header probe) |
| `Discover { input }` | `Vec<DiscoveredItem>` | the input plugin lists the items; the engine fills in `targets` from registry metadata |
| `Select { input, selector: Value }` | `Vec<ItemRef>` | the input plugin, which parses its own selector struct |
| `Locate { from: ItemRef, key }` | `ItemRef` or `MissingDependency` | the plugin owning `from`'s file: it checks its own file, then its cache, then opens other files lazily |
| `ReadItem { item }` | `RawItem { kind, version, header: Bytes, data: ItemData }` | the input plugin; `ItemData` reads byte ranges on demand |
| `Decode { item }` | `Decoded = Arc<dyn Intermediate>` | the decoder for (kind, version) |
| `Encode { item, target: Value, path: Option<String> }` | `Vec<String>` (written paths) | the encoder for target `type` |

- **`OpenedInput`** is an object-safe trait with `id`, `source_files` (every file pulled in: the rpak, its starpaks and patches), `items` and `suggested_path(item)`.
- **Item identity is `ItemRef { file: String, key: String }`,** the file plus the format's internal id: a GUID for RPak, an entry path for VPK, empty for a single-asset file.
  - Some references don't know their file yet. An RPak dependency, for example, is stored only as a GUID and often lives in another pak such as `common.rpak`. Such a reference stays inside the plugin until `Locate` turns it into an `ItemRef`.
- **The pipeline is decode → intermediate → encode.** Hosts can stop after decode to get in-memory data.
- **The intermediate set is open:**
  - `trait Intermediate: Any + Send + Sync { fn type_name(&self) -> &'static str; }`
  - Standard types live in `core::types`: `Texture`, `Model`, `Material`, `Animation`, `Rig`, `Audio`, `Table`, `Text`, `Binary`. They are skeletal at first.
  - A plugin can add its own types.
- **Linking:** an encoder asks for what it needs, e.g. a Cast model encoder sends `Encode{texture, {"type":"png"}}` and gets the written path back. Dedup ensures each thing is produced once, and the cycle guard catches loops. **There is no `with_dependencies` flag:** a request names only the desired output, and converters fetch the dependencies that output needs.
- **Typed helpers** let plugin authors use their own structs for selectors and options while the core only sees JSON: `TypedSelector<T>` and `TypedTarget<T>`, where `T: Serialize + DeserializeOwned`. This is the Rust equivalent of .NET's `IHandler<TSelector>` → `IHandler`.

## 7. I/O and memory rules
- All I/O goes through OpenDAL `Operator`s supplied by the host: **input**, **output** and an optional **cache**.
- **Never load a large file fully into memory unless it's necessary** (see section 1).
  - `OpenInput` reads what's needed to list items, using `read_with(path).range(..)` where the format allows.
  - Item data, especially starpak data, is read by offset and length, in chunks.
- **Caveat to verify per RPak version:** a compressed `.rpak` body may have to be decompressed as one stream. That costs the size of the `.rpak`, not of the starpaks. Stream-decompress it and keep only the tables and pages that are needed.
- **Memory policy:** decoded results live only while a request is in flight (for dedup). Nothing is kept for the whole call. Reuse comes from the plugin cache or from decoding again.
- **Cross-file dependencies** are opened lazily, **only when the current request needs them**. If a file can't be located or opened, that output fails with `MissingDependency{item, file}`.

## 8. Optional plugin cache
- It is an optional `Operator`, so any OpenDAL backend works: a directory, memory, SQLite, …
- Each plugin gets a **namespaced view**, `cx.cache() -> Option<PluginCache>`. The prefix is the plugin id plus its schema version.
  - Methods: `get`, `put`, `remove`, `get_json<T>`, `put_json<T>`.
- **Core never interprets cache contents.** Each plugin decides what helps it decode faster, such as name mappings or indexes.
  - Example: rsx caches GUID → original name and GUID → containing file. An RPak GUID is a hash of the asset path, so names can be recovered and dependencies located.
- **Plugins must work without a cache.** A cache error becomes a `Warning` event and is never fatal.

## 9. Public API (`respak-core::Engine`)
- **Builder:** `Engine::builder().input(op).output(op).cache(op /*optional*/).plugin(p).middleware(m).build()`
- **Methods:**
  - `engine.discover(path, &cancel) -> Result<Discovery>`
  - `engine.convert(&ConvertRequest, &cancel) -> ConvertReport`: each output becomes `Select` + `Encode`. **When one output fails the others continue**, and the report has a result for each output.
  - `engine.decode(path, selector, &cancel) -> Result<Decoded>`: in-memory access.
  - `engine.events() -> broadcast::Receiver<Event>`

JSON shapes. A request is **one input file plus a list of desired outputs**:
```json
{ "version": 1,
  "input": "paks/Win64/common.rpak",
  "format": null,
  "outputs": [
    { "select": { "asset": "mdl/weapons/r97/w_r97.rmdl" }, "target": { "type": "cast" } },
    { "select": { "guid": "0x8A3F12C4D5E6F701" }, "target": { "type": "png", "mips": "all" }, "path": "custom/name.png" }
  ] }
```
- `select` is the input plugin's own struct.
  - RPak: `{asset}`, `{guid}` or `{kind}`. An asset path is hashed to its GUID, so selection works even without names.
  - MDL or DDS: `{}`.
  - VPK: `{entry}`.
- `target` is the encoder's own struct: `type` picks the encoder, and the other fields are its options.
- `path` is optional. By default the input plugin's `suggested_path` is used; for RPak that is the asset path, falling back to the GUID.
- `ConvertReport { outputs: [ Ok{files, dependencies} | Err{error} ] }`
- `Discovery { input, format, items: [ { select, kind, version, name?, targets: [ {type, label, defaults} ] } ] }`. Discovery returns the **same shapes** a request uses, so entries can be copied straight into a request.

Other defaults:
- The input format is auto-detected, and `"format"` overrides it.
- Existing outputs are overwritten.
- Top-level outputs run in parallel, up to a configurable limit.

## 10. Errors
`Error` is cheap to clone (`Arc`). Its variants:
- `NoHandler`, `NotFound`, `Storage(opendal::Error)`
- `UnsupportedVersion{kind,version}`, `InvalidSelector`, `NoMatch`
- `UnknownTarget`, `TargetNotApplicable`, `InvalidOptions`
- `MissingDependency{item,file}`, `DependencyCycle{chain}`
- `Corrupt`, `Cancelled`

## 11. CLI (`respak-cli`)
- `src/lib.rs` holds `async fn run(cli, engine, out) -> Result<ExitCode>`, so it can be tested in-process with `services::Memory`.
- `src/main.rs` runs under `#[tokio::main]`. It:
  - builds `services::Fs` operators from `--root`, `--out` and the optional `--cache`
  - registers the plugins
  - prints events to stderr
  - wires Ctrl-C to the cancel token
- Commands:
  - `formats` lists the registered handlers as JSON.
  - `discover <input>` prints the Discovery JSON.
  - `run <request.json | ->` runs a full request and prints the report.
  - `convert <input> --select '<json>' --target '<json>' [--path P]` is the shorthand for a single output.

## 12. Open questions (deferred, not to be decided alone)
- **Raw-byte caching of the input `Operator`:** the user asked to revisit this later. Ask them.
- **FFI shape for the async API.** "JSON in / JSON out + an event callback" looks natural.
- **Exact fields of the intermediate types,** to be refined with the first real decoders and encoders.
- **Option schemas:** should `TargetInfo` carry a JSON Schema (e.g. `schemars`)?
- **OpenDAL version pinning** and an API-name check.
- **RPak compression layout per version:** confirm the caveat in section 7.

## 13. Background from rsx (format knowledge source)
- **What the rsx export architecture does:**
  - a global `std::map<fourCC, AssetTypeBinding_t>`, where each binding has `exportFunc(asset, setting)` and the format index lives in global state
  - dispatch in `src/core/filehandling/rpak.cpp`, run on a parallel task pool
- **Problems in rsx that respak avoids:**
  - global mutable settings
  - shared dependencies exported several times, concurrently, to the same path
  - the no-GUI build never executes the export tasks
  - exporters write directly to disk, and each one repeats path building
  - failures reported as a bare `bool`
- **Where to look in the rsx repo for format knowledge:**
  - `src/game/rtech/cpakfile.{h,cpp}`: RPak headers v6/v7/v8, asset entries, patches, starpaks
  - `src/game/rtech/starpak_source.h`: the range-reader abstraction for starpaks
  - `src/game/rtech/assets/*.cpp`: per-asset-type parsing and export (texture, material, model, animseq, …)
  - `src/core/mdl/`: model writers (Cast, SMD, QC, RMAX)
  - `src/core/cache/cachedb.h`: the GUID → name/file mapping database
