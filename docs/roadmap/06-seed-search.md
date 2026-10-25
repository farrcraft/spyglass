# 06. Seed search

- Status: Planned
- Depends on: [03](03-structure-placement.md)
- Plans: none yet

## Goal

Describe what you want in a world and get back the seeds that fit it **best**, not just any seed that passes.

## Scope

- `spyglass-search` on `rayon`: parallel search over seed ranges, from the API, the CLI (`spyglass search`) and the viewer.
- **Constraints**, each either **required** (a seed must meet it) or **preferred** (it adds to the seed's score):
  - counts of a structure type within a region or radius, e.g. "at least 3 witch huts within 500 blocks of spawn";
  - groupings, e.g. two or more witch huts close enough to share one AFK spot;
  - biome present within a radius, or covering a share of an area;
  - spawn conditions and slime chunks;
  - distance between features, e.g. "a village within 300 blocks of an ancient city".
- **Ranking:** a score from the preferred constraints, with a top-N result list that updates live while the search runs, and a time or seed-count budget.
- A query language for constraints, usable from files and the CLI, and editable in the viewer.
- Cost ordering: cheap seed-only checks (structure placement over the lower 48 bits) run first, and expensive ones (biomes, then terrain) only on survivors. Structure-only constraints search the lower 48 bits, then expand to full 64-bit seeds.
- Results open straight in the viewer.

## Exit criteria

- [ ] The query language has a spec and an ADR.
- [ ] Every seed the search reports meets all its required constraints, verified against the plain (unoptimized) implementations.
- [ ] Scores are deterministic: the same query and seed range give the same ranked list.
- [ ] Benchmarks exist for the main constraint types.
