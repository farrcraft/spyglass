# 0018. A biome palette of spyglass's own

- Status: Accepted
- Date: 2026-10-25

## Context

The map needs a colour for each biome and each structure set. The game has no such colours: a biome's JSON has water, sky and sometimes grass and foliage colours, but they're for tinting textures and are often missing or close together (most land biomes share one water colour). Grass and foliage colours mostly come from colour-map textures, which are game assets that spyglass doesn't extract.

The project rule is that values which change between game versions are bundle data, not Rust constants. A palette is spyglass's presentation, not a game parameter, but the set of biome ids does change between versions: 26.3 has `dappled_forest` and `sulfur_caves`, for example, and a later version will add more.

Other map tools have palettes, but Amidst's is GPL-licensed, and copying a palette ties spyglass to a choice made for a different set of biomes.

## Decision

We will ship a built-in palette in `spyglass`: a table of colours chosen for spyglass, keyed by biome id (and one by structure-set id for markers). An id the table doesn't have gets a colour derived from a hash of the id, so a new version's biomes still render, distinctly and the same on every run. Nothing in the palette is read from the bundle.

## Consequences

- Every version renders without waiting for a palette update; new biomes look arbitrary until the table gains them.
- Adding a version's new biomes to the table is a small code change, outside the bundle; [specs/render.md](../specs/render.md) lists the rule.
- Colours are ours to choose and to keep readable together (oceans blue, deserts sand, snow white).
- The palette is not user-configurable yet. If that's wanted later, a file can override the table without changing this decision.

## Alternatives considered

- **Colours from the bundle's biome JSON:** too sparse and too similar to tell biomes apart.
- **Extracting the colour-map textures:** game assets, a larger extractor, and the result still isn't a map palette.
- **Hashed colours only:** no table to maintain, but neighbouring biomes would get unrelated colours, and oceans wouldn't look like water.
- **Another tool's palette:** licence constraints (Amidst is GPL), and it fits that tool's biome list, not ours.
