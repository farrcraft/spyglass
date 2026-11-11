# 26. Data packs in generation

- Status: Planned
- Depends on: [04](completed/04-world-saves.md), [08](completed/08-dimensions-and-versions.md)
- Plans: none yet

## Goal

A world whose data packs change world generation gets answers for its packs, not a warning. spyglass keeps parameters in data, so a pack's worldgen files can replace the bundle's as the game's registries replace vanilla's. Today `spyglass world info` flags such worlds as inexact ([world-saves.md](../specs/world-saves.md#warnings)). Horizon applies packs this way ([prior art](../research/prior-art.md)).

## Scope

- **Overlaying packs:** each enabled pack's `data/<namespace>/worldgen/**`, its structure templates and its block, fluid and biome tags, applied over the bundle in `Data.DataPacks.Enabled`'s order, a higher pack's file replacing a lower one's for the same id.
- **What packs add:** new biomes (a colour for the map, [ADR 0018](../adr/0018-biome-palette.md)), structures, features and density functions, as far as spyglass already supports their types. A type spyglass doesn't support is reported, not guessed.
- **The oracle** loads the same packs (`--pack`, as [the test pack](../specs/oracle.md#the-test-pack) does), so pack worlds are checked against the game.
- **Front ends:** `--world` applies a save's packs, `--pack <dir>` does for a seed, and the viewer applies a save's packs and says which.
- **Warnings** narrow to what spyglass can't apply: mods, packs whose worldgen spyglass doesn't support, and missing packs.

Not in scope:

- Mods' code. Only data.
- Packs written for another version than the bundle's: the game refuses those too.

## Exit criteria

- [ ] A fixture pack that overrides a noise, a density function, the noise settings, a placed feature and a biome tag matches the game with the pack, and differs from vanilla without it, in every comparison the oracle makes.
- [ ] A widely used worldgen pack, extracted locally and not committed, is checked against the game for a few seeds, with the result recorded.
- [ ] `spyglass world info` no longer warns about worldgen packs spyglass applies, and says what it applied.
- [ ] The user guide describes data packs.
