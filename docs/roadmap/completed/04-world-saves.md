# 04. World saves

- Status: Completed
- Depends on: [03](03-structure-placement.md)
- Plans: [m04-world-saves](../../plans/completed/m04-world-saves.md)

## Goal

Point spyglass at a local world save and get answers for that world: its seed, version and settings, and where things are relative to where you are.

## Scope

- `spyglass-world`: reads the save's gzip NBT files.
  - **26.x layout** (seen in real 26.3 saves): `level.dat` has `Data.DataVersion` (5023) and `Data.DataPacks.Enabled`, but **not the seed**. The seed and generator settings are in `data/minecraft/world_gen_settings.dat` (`data.seed`, `data.dimensions.<dim>.generator`).
  - Region files are under `dimensions/minecraft/<dimension>/region/`.
  - Older saves keep the seed in `level.dat` (`Data.WorldGenSettings`). Supporting both layouts is in scope.
- Finds saves in known launcher locations (official launcher `saves/`, Modrinth profiles) and accepts any path.
- Maps the save's data version to a game version, using a data-version table recorded in each extracted bundle.
- Opens a session from a save through the public API, so every locate query from [03](03-structure-placement.md) works on it.
- CLI: `spyglass world info <save>` and `spyglass locate --world <save> ...`.
- Clear handling of worlds spyglass can't predict exactly: unsupported presets, datapacks that change worldgen, or a version with no bundle. Each gets a specific warning, not wrong answers.

## Exit criteria

- [x] Seed, version and preset are read correctly from saves made in the target version.
- [x] Locate results for a real save match what's in that world, spot-checked in game.
- [x] Every unsupported case above is detected and reported.
- [x] A spec covers which `level.dat` fields are read and how version mapping works.

## Open questions

- ~~Upgraded worlds.~~ Resolved: spyglass counts the chunks older versions generated and warns. Reading the structure starts stored in chunks, for exact answers where the world exists, is a later extension.
- ~~Which NBT crate.~~ Resolved by [ADR 0013](../../adr/0013-structure-templates-and-nbt.md): spyglass's own reader, which this milestone also reads saves and region files with.
