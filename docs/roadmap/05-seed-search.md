# 05. Seed search

- Status: Planned
- Depends on: [03](03-structure-placement.md)
- Plans: none yet

## Goal

Find seeds that satisfy a set of conditions, fast.

## Scope

- `spyglass-search` on `rayon`: parallel search over seed ranges.
- Composable filters (structure near point, biome at point, spawn conditions, slime chunks), ordered cheapest first so expensive checks run on few candidates.
- A filter language, usable from a CLI and later from the viewer.
- Structure-only filters search the lower 48 bits first, then expand to full 64-bit seeds.

## Exit criteria

- [ ] Filter language has a spec and an ADR.
- [ ] Every seed the search reports passes all its filters, verified against the reference implementations.
- [ ] Benchmarks exist for the main filter types.
