# 08. Nether, End and multiple versions

- Status: In progress
- Depends on: [07](completed/07-terrain-and-validity.md)
- Plans: [m08-dimensions](../plans/m08-dimensions.md); m08-versions to come

## Goal

Cover all three dimensions and more than one game version.

## Scope

- Nether and End biomes and structures, available through locate queries, search constraints, world saves and the viewer, with a dimension switch.
- Cross-dimension constraints, e.g. "a nether fortress within 100 Nether blocks of the Overworld spawn's portal position".
- Support for several game versions side by side, each with its own bundle.
- A documented process for adding a new version: extract, diff, fix what changed.

## Exit criteria

- [ ] Nether and End outputs match the oracle.
- [ ] At least two versions pass the full oracle suite in CI.
- [ ] A spec or guide documents how to add a version, and has been followed at least once.
