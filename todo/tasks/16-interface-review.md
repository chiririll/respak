# 16 · Interface review with the user
**Status:** blocked (waits for 15) · **Depends on:** 15

## Goal
The user reviews the phase 1 interfaces. This is a user gate, not agent work.

## How
- Walk through the review with the user. Present unclear points as questions; don't decide them alone.
- Record every requested change in the relevant task file and in PROJECT.md, and apply it.
- Items already flagged for review:
  - the `Detect` / `Probe` API and the score convention
  - the `rpak.asset` fields and the rpak volume layout (`.mem`)
  - capability method sets (`Bitmap`, pixel formats)
  - png/dds option names
  - middleware order

## Done when
The user explicitly approves the interfaces.
