# 16. Search within a seed

- Status: Completed
- Depends on: [15](15-nearest-locate.md)
- Plans: [m16-search-within-a-seed](../../plans/completed/m16-search-within-a-seed.md)

## Goal

Find places in one seed, not only seeds: "double witch huts that share an AFK spot", "an ocean monument with only deep ocean around it", "a pillager outpost on flat ground", "the nearest ice spikes". The seed search's constraints already describe surroundings. This milestone points them at candidate places in one seed, nearest first, and steps through the matches on the map.

## Scope

- **The language:** a target (a structure, a biome, or each chunk) and constraints relative to it, with a place keyword such as `here` for "around the target". As far as possible, the seed search's constraints work unchanged. The syntax and how the two kinds of query coexist get an ADR before code, extending [ADR 0020](../../adr/0020-search-query-language.md).
- **Terrain conditions:** a height range and a flatness measure over an area, from the height maps the library already computes. These are the conditions for farm and base sites.
- **The library:** a search over one session from a point, outward to a radius, nearest first, returning each match with its constraint results, in the same form the seed search uses. It is cancellable, and it reports progress. It is deterministic for a given query, seed and origin.
- **The CLI:** `spyglass find`, or a mode of `search`, with `--json`.
- **The viewer:** a mode of the Search tab for the seed shown, with results ringed on the map, a count, next and previous (keyboard too), and the opened match's constraints drawn as they are for a seed search hit.
- **Presets:** a few starting queries in the viewer, written in the language, so they're a way to learn it rather than a separate feature: double huts, a monument in deep ocean, a flat outpost, a rare biome nearby.
- **The spec:** search.md, or a new spec beside it, covers the target, the order and the terrain conditions.

Not in scope:

- Features as targets or conditions (dungeons, geodes and the rest). They join the language when [19](../19-features.md) delivers them.
- Searching across seeds and within them in one query.

## Exit criteria

- [x] Each preset finds its first match on a fixed seed, and the matches agree with `structures_in`, `biome_at` and `height_map` over the same places (tests on fixture bundles, and ignored ones on the real bundle).
- [x] Results are the same for the same query, seed and origin, whatever the thread count.
- [x] A search over 4,096 blocks around the origin for a two-structure cluster finishes in a time recorded in the plan, and the viewer stays responsive while it runs.
- [x] `cargo xtask check` passes.
