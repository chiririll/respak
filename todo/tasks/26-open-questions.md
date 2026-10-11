# 26 · Open questions (resolve with the user, not alone)
**Status:** blocked

| Question | Context |
|---|---|
| Raw-byte caching of the input `Operator` | The user explicitly asked to come back to this later. Ask before designing anything. |
| FFI shape for the async API | "JSON in / JSON out + an event callback" looks natural. How should a C host drive tokio? |
| Exact intermediate type fields | Refine with the first real decoders and encoders (tasks 22, 23, 25). |
| Option schemas | Should `TargetInfo` carry a JSON Schema (e.g. `schemars`), or only defaults? |
| OpenDAL version | It is pre-1.0. Pin it, and plan how upgrades are handled. |
| RPak compression per version | Confirm or correct the caveat in PROJECT.md §7 (task 20). |
| Codec licensing | Oodle and similar codecs: how to obtain or handle them legally. |
