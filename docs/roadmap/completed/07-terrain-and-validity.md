# 07. Terrain, structure validity, jigsaw

- Status: Completed
- Depends on: [03](03-structure-placement.md)
- Plans: [m07-terrain-and-validity](../../plans/completed/m07-terrain-and-validity.md)

## Goal

Know whether structures actually generate, and how big they are.

## Scope

- `spyglass-density`: full noise router evaluation.
- `spyglass-terrain`: the generator's heightmaps and block columns (density and aquifer). Surface rules (in 26.3, the material rule) are out of scope: no height query uses them, and they belong with features and the final spawn block in a later milestone.
- `spyglass-structure`: terrain-dependent validity checks and optional jigsaw assembly for exact bounds.
- Locate queries and search constraints gain a "confirmed" mode that applies these checks.
- Oracle extended to dump heightmaps and structure bounds.

## Exit criteria

- [x] Heightmaps match the oracle for the seed set.
- [x] Structure starts after validity checks, and their bounds where jigsaw is used, match the oracle.
- [x] Evaluation is fast enough to use as a late-stage search constraint; target set in the plan.
