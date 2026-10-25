# 07. Terrain, structure validity, jigsaw

- Status: Planned
- Depends on: [03](03-structure-placement.md)
- Plans: none yet

## Goal

Know whether structures actually generate, and how big they are.

## Scope

- `spyglass-density`: full noise router evaluation.
- `spyglass-terrain`: heightmaps and surface rules.
- `spyglass-structure`: terrain-dependent validity checks and optional jigsaw assembly for exact bounds.
- Locate queries and search constraints gain a "confirmed" mode that applies these checks.
- Oracle extended to dump heightmaps and structure bounds.

## Exit criteria

- [ ] Heightmaps match the oracle for the seed set.
- [ ] Structure starts after validity checks, and their bounds where jigsaw is used, match the oracle.
- [ ] Evaluation is fast enough to use as a late-stage search constraint; target set in the plan.
