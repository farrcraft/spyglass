# 31. What spyglass read from a save

- Status: In progress
- Depends on: [25](completed/25-what-the-save-knows.md), [26](completed/26-data-packs.md)
- Plans: [m31-save-inspector](../plans/m31-save-inspector.md)

## Goal

Show what spyglass read from a world save, and how it used it, so a user can see why an answer for their world is what it is. Today a save's reading surfaces only as warnings ([world-saves.md](../specs/world-saves.md#warnings)). This is an explanation of spyglass's own answers, not a general NBT browser: tools such as NBTExplorer and Amulet already browse and edit saves.

## Scope

- **The settings.** The values spyglass took from `level.dat` (seed, version, generator and preset per dimension, spawn, world border, single player), each with where it came from and what it decides.
- **The data packs.** Each pack the save enables, in order, and what it changes that spyglass reads, as [ADR 0072](../adr/0072-data-packs-as-layers.md) layers them.
- **What the save holds.** How much of each dimension is generated, which versions generated it, and how many structure starts and opened containers came from the save rather than the seed ([ADR 0071](../adr/0071-answers-from-a-save.md)).
- **The warnings**, beside the values they're about.
- **The library first.** The report is a value the public API returns, shown by a viewer panel and by `spyglass world info`, which grows from today's summary into the full report, with `--json`.

Not in scope:

- Browsing a save's NBT in general, or any file spyglass doesn't use for its answers.
- Writing to a save, ever ([world-saves.md](../specs/world-saves.md)).

## Exit criteria

- [ ] `spyglass world info` and the viewer's panel give the same report for a real 26.3 save, one with data packs and one with chunks from an older version.
- [ ] Every warning a save can raise appears in the report, beside what it's about (a test per warning).
- [ ] The user guide's [your own worlds](../guide/src/your-worlds.md) page describes the report.
