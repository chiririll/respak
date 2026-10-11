# 23 · Implement PNG/DDS encoders (TDD)
**Status:** blocked · **Depends on:** 19

## Goal
Implement the `png` and `dds` encoders in `respak-image` for `core::types::Texture`.

## Notes
- **Use existing crates** for PNG writing and pixel-format decoding (e.g. the `image` / `png` crates, a DDS crate, and a BCn decoder crate). Evaluate them and propose the choice to the user before adding dependencies. Don't hand-write codecs.
- CPU work goes through `cx.blocking`. Output is written through the output `Operator`.

## Tests
_To be filled in by task 17._ Test with synthetic `Texture` values; there is no RPak dependency.
