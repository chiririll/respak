# 13 · Plugin skeleton: respak-image
**Status:** todo · **Depends on:** 11 · **See:** PROJECT.md §3, §6

## Goal
Show how an output plugin plugs into the core. This is a review aid only, and bodies are `todo!()`.

## Scope
- `ImagePlugin: Plugin` registers:
  - `PngEncoder: Handler<Encode>`, with `target_type = "png"`, `accepts = [Texture::TYPE_NAME]`, and options `PngOptions { mips: Highest | All }` via `TypedTarget`
  - `DdsEncoder: Handler<Encode>`, with `target_type = "dds"` and options for variants (highest mip / all mips / mipmapped). Final names are decided in review.
- Encoders get the decoded data by sending `Decode { item }` through the mediator. They run CPU work via `cx.blocking`, and write through the output `Operator`.
- **Depends only on `respak-core`.** It must not know about `respak-rpak`.

## Acceptance
- It compiles and registers with an `Engine` builder.
