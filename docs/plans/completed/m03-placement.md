# m03. Structure placement and the biome check

- Status: Completed
- Milestone: [03. Structure placement and locate queries](../../roadmap/03-structure-placement.md)
- Started: 2026-10-25
- Completed: 2026-10-25

## Goal

For any seed, every Overworld structure set's placement chunks and, for each, which structure the game would try and whether it passes the biome check, matching the oracle. Also slime chunks. Covers the milestone's first exit criterion, the slime half of the second, and the placement spec of the fourth.

The first of three plans for milestone 03, followed by [m03-spawn](../m03-spawn.md) and [m03-api-cli](../m03-api-cli.md). It needs only the completed milestone 02 crates.

## Facts established up front

Checked against `bundles/26.3/` and the 26.3 jar (`javap`) on 2026-10-25.

- **Structure sets.** `worldgen/structure_set/` has 21 sets. All but `strongholds` are `minecraft:random_spread` with `salt`, `spacing`, `separation` and optionally `spread_type` (`triangular`: `ocean_monuments`, `end_cities`), `frequency` with `frequency_reduction_method` (`legacy_type_1`: pillager outposts, `legacy_type_2`: buried treasure, `legacy_type_3`: mineshafts), `exclusion_zone` (`pillager_outposts`: 10 chunks from `villages`) and `locate_offset` (`buried_treasures`: `[9, 0, 9]`). `strongholds` is `minecraft:concentric_rings` with `count` 128, `distance` 32, `spread` 3, `salt` 0 and `preferred_biomes: "#minecraft:stronghold_biased_to"`. Several sets list more than one weighted structure (`villages`: 5, `abandoned_camp`: 18, `ruined_portals`: 7, `mineshafts`: 2).
- **Structures.** `worldgen/structure/` has 52 files. Each has `type` and `biomes` (a biome tag such as `#minecraft:has_structure/village_plains`); jigsaw structures (28) add `start_height`, `project_start_to_heightmap`, `start_pool`, `size` and `max_distance_from_center`. Biome tags are in `tags/worldgen/biome/`, and may reference other tags.
- **Which sets apply.** `ChunkGeneratorStructureState.createForNormal` keeps only sets with a structure whose biomes intersect the biome source's possible biomes (`hasBiomesForStructureSet`). For the overworld that drops the Nether and End sets. `ruined_portals` stays, but its `ruined_portal_nether` member can never pass.
- **Placement.** `StructurePlacement.isStructureChunk(state, x, z)` is the placement test plus `applyAdditionalChunkRestrictions` (frequency) and the exclusion zone (`ChunkGeneratorStructureState.hasStructureChunkInRange`, placement only, no biomes). `getLocatePos` adds `locateOffset` to the chunk origin.
- **Rings.** `generateRingPositions` seeds a `RandomSource` with `concentricRingsSeed`. It draws angles and distances with `nextDouble`, uses `Math.cos`/`Math.sin` and `Math.round`, and moves each position with `BiomeSource.findBiomeHorizontal` (radius 112, at the chunk centre, through the forked random) towards a `preferred_biomes` biome. It doesn't read the dimension origin (`origin` is only returned by `getDimensionOrigin`).
- **Selection.** `ChunkGenerator.tryGenerateStructure` calls `Structure.generate` for an entry; for sets with several entries, a weighted pick that removes a failed entry and retries. `GenerationContext.makeRandom` seeds a `WorldgenRandom(LegacyRandomSource)` with `setLargeFeatureSeed(seed, chunkX, chunkZ)`.
- **The biome check is in 3D.** `GenerationContext.isValidBiome(stub)` resolves the biome at `QuartPos.fromBlock` of the stub's x, **y** and z. For most types the stub's y comes from a heightmap (`onTopOfChunkCenter`, `JigsawStructure` with `project_start_to_heightmap`), so the exact check needs terrain, which is milestone 07. Others take y from data or constants (`start_height` without projection, e.g. ancient city and trial chambers), or check an area (`OceanMonumentStructure` calls `BiomeResolver.getBiomesWithin` around sea level). `GenerationContext.couldStructureExistInColumn` and `couldValidBiomeExistInTerrainColumn` (woodland mansion, end city) test a y range. A climate-only "could any biome in this column pass" is a necessary condition for every type.
- **Slime chunks.** `WorldgenRandom.seedSlimeChunk(x, z, seed, salt)` then `nextInt(10) == 0`. The salt is `987234911L` (`Slime.checkSlimeSpawnRules`).

### Where each type checks its biome

From `findGenerationPoint` per type (26.3 `javap`, 2026-10-25). "Pre-check" is `couldValidBiomeExistOnTopOfChunkCenter`: some quart in the full-height column at the chunk centre (`minY - 1` to `maxY`) is in the structure's biomes. It's climate-only and exact, so it rejects chunks soundly.

| Type                                                      | Check position                      | y from                                                   | Terrain?   | Exact now                                                                                                                                           |
| --------------------------------------------------------- | ----------------------------------- | -------------------------------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| stronghold                                                | (minX, 0, minZ)                     | constant                                                 | no         | yes                                                                                                                                                 |
| buried treasure, ocean ruin, shipwreck                    | chunk centre                        | `OCEAN_FLOOR_WG` (beached shipwreck: `WORLD_SURFACE_WG`) | yes        | pre-check only                                                                                                                                      |
| desert pyramid, jungle temple, swamp hut, igloo           | chunk centre                        | `WORLD_SURFACE_WG`                                       | yes        | pre-check only; pyramids and temples are also rejected if their lowest corner is below sea level (terrain)                                          |
| ocean monument                                            | chunk centre                        | `OCEAN_FLOOR_WG`                                         | yes        | `getBiomesWithin(minX + 9, seaLevel, minZ + 9, 29)`, which requires every biome to be in `#required_ocean_monument_surrounding`, then the pre-check |
| woodland mansion                                          | (minX + 7, lowest corner, minZ + 7) | `WORLD_SURFACE_WG`, 5 × 5 corners by rotation            | yes        | full-column test at (minX + 7, minZ + 7); also rejected if lowest y < 60 (terrain)                                                                  |
| jigsaw (villages, outposts, abandoned camps, trail ruins) | start piece's bounding-box centre   | start height + `WORLD_SURFACE_WG`                        | yes        | x and z, from the start template (bundled since ADR 0013); then the column test at them                                                             |
| jigsaw (ancient city, trial chambers)                     | start piece's bounding-box centre   | `start_height` (trial chambers: uniform −40..−20)        | no         | yes, from the start template                                                                                                                        |
| ruined portal                                             | template bounding-box centre        | `findSuitableY` over noise columns                       | yes        | no: needs its setup and template draws ported, then terrain; unfiltered for now                                                                     |
| mineshaft (normal / mesa)                                 | (midX, 50 + dy, minZ)               | dy from the full piece layout (mesa: terrain)            | normal: no | the column test at (midX, minZ); dy needs the piece layout (milestone 07)                                                                           |

The piece RNG is drawn after the biome check (`GenerationStub` builds its pieces lazily), except for mineshafts, which lay out their pieces first.

## Approach

1. **Loading** (`spyglass-structure`, typed per [ADR 0010](../../adr/0010-typed-loading-of-worldgen-json.md)): structure sets, the structures they name (only the fields placement and the biome check need), and biome tags with nested references, resolved to sets of `BiomeId`s for the source's parameter list. Unknown placement or structure types are load errors, not silent skips.
2. **Placement**: `RandomSpread::potential_chunk(seed, cell)` and `is_structure_chunk(x, z)` with the spread types, the four frequency reducers and exclusion zones; `ConcentricRings::positions()`, computed once per seed. Placement depends only on the seed (and biomes, for rings), so it's cheap and comes first, as [architecture.md](../../architecture.md) says.
3. **`findBiomeHorizontal`** in `spyglass-biome`: the square scan with reservoir selection over the given random, using the point lookup. Ties: the predicate takes the lookup's answer, and the tie rule from [biome-lookup.md](../../specs/biome-lookup.md#ties) applies. Report when a ring position depended on a tied cell.
4. **Biome check**: classify each structure type by where its check samples. The result per (chunk, structure) is `Rejected` (fails at every position the game could use), `Passes` (passes at a position that doesn't depend on terrain) or `Candidate` (passes somewhere in the column the heightmap could pick; needs milestone 07 to resolve). Multi-entry sets run the game's weighted pick with this three-way answer, so a set result is either one structure or a list of candidates in the game's try order.
5. **Exact check given a position**: `biome_check_at(structure, block_pos)`, so that once terrain supplies y, or the oracle does, the check itself is exact.
6. **Slime chunks**: `is_slime_chunk(seed, x, z)`.
7. **Oracle**: a `structures` subcommand that, for a seed and chunk region, writes every applicable set's placement chunks, the game's selection and stub position for each, the biome at that position and whether a start was produced, plus the ring positions. It also covers slime chunks (probe file) and the golden set.

8. **Templates** (added after the template question below): bundle `data/minecraft/structure/` and read it with a small NBT reader ([ADR 0013](../../adr/0013-structure-templates-and-nbt.md)), so jigsaw start positions are exact.

## Tasks

- [x] Verify what isn't settled above against the jar: `concentricRingsSeed`'s source (the level seed), the frequency reducers' seeding, triangular spread, the weighted-pick RNG, and each structure type's check position (`findGenerationPoint` per type, below). Record the table in the spec.
- [x] `java.lang.Math.sin`/`cos`: neither Rust's nor fdlibm matches HotSpot's intrinsic (measured), so results within a few ulps of a rounding boundary are flagged as platform-dependent ([ADR 0012](../../adr/0012-java-trig-results-that-depend-on-the-platform.md)).
- [x] `spyglass-structure`: load structure sets, structures and biome tags from the bundle; filter sets by the source's possible biomes. Unit tests on hand-written bundles, including nested tags, optional entries, tag cycles and unknown types.
- [x] `spyglass-structure`: random-spread placement (linear, triangular), frequency reducers, exclusion zones, `locate_offset`. Unit tests on hand-written parameters, and negative cells (`floorDiv`).
- [x] `spyglass-biome`: `find_biome_horizontal`, checked through the ring positions against the oracle. Tied cells are replayed in every combination, up to 12.
- [x] `spyglass-structure`: concentric-ring positions.
- [x] `spyglass-data`: bundle structure templates; `nbt` reader with gzip through `flate2` (ADR 0013).
- [x] `spyglass-structure`: template pools, templates and jigsaw start positions (start height, rotation, start element, named start jigsaw, centre, padding).
- [x] `spyglass-structure`: the biome check with `Rejected` / `Passes` / `Candidate` for every type, weighted selection for multi-entry sets (`Starts::select`), and `Starts::check_at` for a known position.
- [x] `spyglass-structure`: slime chunks.
- [x] Oracle: `structures` subcommand (slime chunks are in the same files, not a probe), part of `golden`. Regions: origin, far and border, 32 × 32 chunks or more. All 128 ring positions per seed.
- [x] Ignored golden test (`crates/spyglass-structure/tests/golden.rs`).
- [x] `docs/specs/structure-placement.md`.
- [x] `docs/specs/oracle.md`: structure golden files, their format and the Rust-side comparison.
- [x] Confirm the amended milestone exit criteria, and accept or revise ADRs 0012 and 0013. Approved and accepted on 2026-10-25.

## Verification

Done on 2026-10-25 for 26.3, all 10 golden seeds:

- Unit tests: 15 in `spyglass-structure`, 3 in `spyglass_rng::worldgen` and 5 in `spyglass_data::nbt`.
- Golden test: equal placement chunks for every applicable set in every region (6,056); equal slime chunks; the game's biome at all 2,427 of its check positions is among ours, and the check agrees wherever the cell isn't tied; all 682 jigsaw check positions equal the game's; every start the game made is in our selection; all 175 starts we call certain were made.
- Rings: 1,280 positions; 304 flagged (tie- or platform-dependent), and the 61 that differ from the game are all flagged.
- `cargo xtask check` passes.

## Durable outputs

- [x] `docs/specs/structure-placement.md`
- [x] `docs/specs/oracle.md`: structure golden files
- [x] `docs/specs/README.md`: list the new spec
- [x] `docs/specs/bundle-format.md`: the template directory and NBT
- [x] `docs/specs/rng.md`: worldgen seeding (`WorldgenRandom`)
- [x] [ADR 0012](../../adr/0012-java-trig-results-that-depend-on-the-platform.md): Java trig results that depend on the platform
- [x] [ADR 0013](../../adr/0013-structure-templates-and-nbt.md): structure templates in bundles, read with our own NBT reader
- [x] `docs/architecture.md`: crate dependencies, template data
- [x] `docs/getting-started.md`: running the structure comparison

## Open questions

- **Exit criterion wording.** Resolved: amended in the milestone to: placement exact for every set; the biome check exact where its position doesn't depend on terrain, and exact at the oracle's check positions everywhere; otherwise candidates that always include the game's start. The golden test checks exactly this. Approved.
- **Ruined portals** are unfiltered candidates. Their x and z need the setup, template, rotation and mirror draws ported (all from data and templates); then the column test there would reject soundly. A follow-up, here or in milestone 07.
- **Ring ties.** Resolved: replayed, and reported as `tie_dependent` only if a replay moves the position. About 24% of rings are still flagged; comparing chunks rather than quart positions would flag fewer.

## Notes

- 2026-10-25: Plan drafted.
- 2026-10-25: Started. `spyglass_rng::worldgen` has `WorldgenRandom`'s seeding (spec'd in [rng.md](../../specs/rng.md)). `spyglass-structure` loads the 21 sets of 26.3; 18 apply to the overworld. It also has placement, frequency reducers, exclusion zones, rings and slime chunks. `spyglass-biome` gained `find_biome_horizontal`.
- 2026-10-25: Two surprises. **Trig:** HotSpot's `Math.sin` matches neither fdlibm nor Rust's (each about 2% of inputs off by 1 ulp), so the game is itself platform-dependent; see ADR 0012. **Biome check:** almost every Overworld type's check position needs terrain or structure-template NBT (table above).
- 2026-10-25: Asked about templates; the answer was to bundle them now (ADR 0013). In 26.3, palette entries name blocks by `id`, not the older `Name`, and every template is saved at the current data version. Jigsaw start positions then matched the game first time, ancient cities included once palette ids were read correctly.
- 2026-10-25: The harness needed the static registries' tags bound (`PendingTags.apply`) for structure pieces, and prints stack traces to the raw stdout, because the game routes `System.err` through its logger.
- 2026-10-25: Rings take about 2.4 s per seed (128 × 57 × 57 point lookups, release build). The first tie flag marked any ring with a disagreeing tied cell (524 of 1,280). Replaying the combinations brought it to 304, with every mismatch still flagged.
- 2026-10-25: Completed. The amended exit criteria and ADRs 0012 and 0013 were approved.
