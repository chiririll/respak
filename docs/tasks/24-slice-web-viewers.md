# 24 · Slice: web viewers
**Status:** in-progress (built; demo pending) · **Depends on:** 23 · **See:** [webui.md](../design/webui.md) §7

## Goal
Selecting a file shows it: images (with mip / layer selection), `uimg` atlases with sprite outlines, audio in a player, and a metadata panel for everything else.
```
GET /api/preview?path=…            # viewer data
```

## Changes
1. `GET /api/entry` already reports format, version, score, targets; add the **viewer kind** derived from capabilities (`Bitmap`, `Audio`, `Metadata`), never from the extension.
2. `GET /api/preview`: converts on demand (bitmap → PNG for a chosen mip and layer, audio → WAV) with no write to the output directory and no cache. Ranged reads only: a preview must not pull a whole starpak.
3. Frontend viewers: image (zoom, mip / layer selector, alpha background), atlas (sprite outlines from `uimg.json`, click shows the sprite), audio (`<audio>`), metadata (key/value, dependencies as links that select the dependency in the tree), fallback (metadata + hex of the first bytes **(?)**).
4. Errors from the engine show in the viewer, with the kind and message.

## Leaves out
Video (`bik`) and 3D (mesh, skeleton, animation): metadata and convert only. A preview cache.

## Open
- Hex fallback: yes or metadata only.
- Large textures: downscale on the server or let the browser handle full size.

## Proposed tests (for the gate)
- Preview of a synthetic texture returns a PNG with the requested mip's dimensions; an invalid mip → 400.
- Preview of a synthetic Miles bank entry returns a valid WAV.
- Atlas response lists sprites with rectangles that stay inside the atlas.
- A preview reads only the byte ranges it needs (byte-counting `Operator`, like the starpak test).
- Viewer kind follows capabilities: an unknown extension with a detected `txtr` gets the image viewer.

## Built
Image (mip selector, zoom, pan), atlas (sprite rectangles), audio (waveform, tracks, WAV download), text (any `*json` / `*csv` / `txt` target), metadata and hex of the first KiB for real files. The viewer is chosen from the entry's targets (`uimg.json` → atlas, `png` → image, `wav` → audio, a text target → text), i.e. from what its capabilities can become, never from the extension.
- Previews are converted by a second engine whose output is memory (`build_engine_with`), so nothing reaches the output folder. A small in-memory cache (12 entries, 64 MiB) keeps the last previews so switching a mip back or fetching the two halves of an atlas does not convert twice; this deviates from "no cache" in webui.md §9 and is an easy thing to drop.
- `Engine::open_read(path)` (core) opens and identifies an entry so its capabilities (bitmap shape, metadata, audio tracks) can be shown.
- Not done: layers other than 0 (the PNG target writes layer 0), downscaling (the browser handles full size), video and 3D.
