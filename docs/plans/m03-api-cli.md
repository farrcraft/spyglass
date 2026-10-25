# m03. Public API, locate queries and the CLI

- Status: In progress
- Milestone: [03. Structure placement and locate queries](../roadmap/03-structure-placement.md)
- Started: 2026-10-25
- Completed: —

## Goal

The `spyglass` facade crate, with one session object and locate queries, and the `spyglass` command over it, with human-readable and `--json` output. Covers the milestone's third, fifth and sixth exit criteria. Completing this plan completes the milestone.

The last of three plans for milestone 03. Needs [m03-placement](completed/m03-placement.md); the spawn command needs [m03-spawn](completed/m03-spawn.md).

## Facts established up front

Checked against the 26.3 jar (`javap`) on 2026-10-25.

- **`/locate structure`** calls `ChunkGenerator.findNearestMapStructure`. It groups the target structures by placement and asks each placement for its nearest generated structure: `getNearestGeneratedStructure` over the ring positions for concentric rings, or a ring-by-ring search over placement cells for random spread. It keeps the closest by `BlockPos.distSqr`. "Generated" means a real start, through `StructureManager`, so the game's answer includes terrain-dependent validity.
- **Search order matters.** The random-spread search stops at the first ring of cells with a hit, so `/locate`'s answer isn't always the Euclidean nearest. Agreeing with `/locate` means porting its search order and distance measure, not computing a true nearest.

### The searches in detail

From `javap -c` of 26.3, 2026-10-25.

- **`LocateCommand`**: `findNearestMapStructure(level, holders, pos, 100, false)` for structures; `ServerLevel.findClosestBiome3d(predicate, pos, 6400, 32, 64)` for biomes. `pos` is the command source's block position.
- **`findNearestMapStructure`**: groups the holder set's structures by placement (`getPlacementsForStructure`, into an insertion-ordered `Object2ObjectArrayMap`). Concentric-ring placements are searched first, each keeping the best by `pos.distSqr(locatePos)` (strict `<`). Random-spread placements then share one ring loop, `r = 0..=100`: in ring `r`, each placement's `getNearestGeneratedStructure` result replaces the best if strictly closer; after a ring in which any placement found something, the best is returned. So ring results compete with the first random-spread ring that hits.
- **Random spread, ring `r`**: `dx` from −r to r outer, `dz` inner, only cells on the ring's edge (`|dx| == r` or `|dz| == r`). The cell is `getPotentialStructureChunk(seed, chunkX + spacing·dx, chunkZ + spacing·dz)`, with `chunkX` and `chunkZ` from `pos >> 4`. The **first** generated hit in that order is returned, not the nearest in the ring.
- **Concentric rings**: every ring position, keeping the nearest generated one by distance from the chunk's `(x·16 + 8, 32, z·16 + 8)` (strict `<`, first wins).
- **"Generated"** (`getStructureGeneratingAt`, over the placement's structures in holder-set order): `StructureCheck.checkStart` returns not present if `applyAdditionalChunkRestrictions` (frequency) fails or `findValidGenerationPoint` is empty; otherwise the chunk is generated to `STRUCTURE_STARTS` and the stored start for that structure must be valid. So the answer is the game's real choice in that chunk, weighted pick, exclusion zone and terrain included. The result is `getLocatePos(chunk)`.
- **Distance**: `Vec3i.distSqr`, in `double`: `dx·dx + dy·dy + dz·dz`.
- **`findClosestBiome3d(pos, 6400, 32, 64)`**: nothing if no possible biome matches. Otherwise `BlockPos.spiralAround(ZERO, 6400 / 32 = 200, EAST, SOUTH)` offsets: the centre, then E, S, W, W, N, N, E, E, E, … (leg `k` of 0 to 4·200 yields `k / 2 + 1` positions, each moved in the direction of the leg before), covering the 401 × 401 square exactly and ending at (200, −200). For each column at `pos + 32·offset`, y values from `Mth.outFromOrigin(pos.y, minY + 1, maxY + 1, 64)`: the clamped start, then alternately up and down, one way only once the other is out of range. The first quart `(x >> 2, y >> 2, z >> 2)` whose biome matches wins, at block `(x, y, z)`.
- **Text seeds** (`WorldOptions.parseSeed`): `trim()`; empty means a random seed; `Long.parseLong`, else `String.hashCode` widened to `long`.

### Decisions

Agreed on 2026-10-25 (the plan's open questions):

- **Search range**: the game's 100 rings by default, so answers agree with `/locate`; `--radius` changes it.
- **Candidates**: a locate answer is a list in the game's search order, each entry labelled certain or candidate, up to and including the first certain one (or a cap). The game's answer is always in the list when it ends in a certain entry.

## Approach

1. **ADR** for the public API: a `Session` (bundle + seed + dimension; later, a world save) that owns the loaded sources and exposes queries; front ends depend only on `spyglass`. It also covers the library-first split between the API, the CLI and the GUI ([architecture.md](../architecture.md#front-ends)), result types that carry `Candidate` vs confirmed and tie information, and errors. Written before the crate.
2. **`crates/spyglass`**: `Session::new(bundle, seed, dimension)`, `biome_at`, `structures_in(set or structure, area)`, `nearest_structure(kind, from)`, `structures_within(kind, from, radius)`, `nearest_biome(biome, from)`, `is_slime_chunk`, `spawn_origin`. Locate queries follow the game's search order. Results are labelled candidates where the biome check needs terrain.
3. **`crates/spyglass-cli`** (binary `spyglass`, `clap`): `spyglass locate structure|biome|slime|spawn`, with `--version`, `--seed` (text seeds hashed as the game does), `--from x,z`, `--radius` and `--json`. The bundle comes from `bundles/<version>/`, or `--bundle`. The bin target sets `doc = false` so `cargo doc` doesn't collide with the `spyglass` library.
4. **`--json` spec**: one documented, versioned schema per command, with a `format` field, because scripts depend on it.
5. **Spot-checks against `/locate`**: `findNearestMapStructure` needs a live `ServerLevel`, whose chunk generation decides "generated", and the standalone harness doesn't build one. So `cargo xtask oracle locate` runs the real dedicated server from the same jar on a throwaway world per seed, sends `execute positioned … run locate …` on its console, and records the answers. An ignored test compares them with the API.
6. **Viewer**: switch `spyglass-viewer` from the lower crates to `spyglass`.

## Tasks

- [x] Disassemble `findNearestMapStructure`, both `getNearestGeneratedStructure` overloads and `BiomeSource.findClosestBiome3d` (radius, steps, order) used by `LocateCommand`.
- [x] ADR: public API shape and the library-first split ([ADR 0014](../adr/0014-public-api-world-and-session.md), Proposed), and [ADR 0015](../adr/0015-locate-checks-on-a-real-server.md) (Proposed) for checking `/locate` on a real server.
- [x] `spyglass-biome`: `find_closest_biome_3d` with `spiral_around` and `out_from_origin`; `find_biome_horizontal` returns the alternatives its tie replays find.
- [x] `spyglass-structure`: `StructureSets::resolve` (ids and tags), `Starts::locate`; ring positions computed lazily per set, and carrying alternative chunks (ties and platform rounding).
- [x] `crates/spyglass`: `World`, `Session` and queries. Unit tests cover seeds and the shown distance; the queries need a whole dimension's data, so they're covered by the CLI tests and the `/locate` comparison rather than synthetic bundles.
- [x] `crates/spyglass-cli`: commands, human output and `--json`. Integration tests running the binary on a bundle (ignored when no bundle is present).
- [x] `docs/specs/cli-json.md`: the JSON output format. `docs/specs/locate.md`: the searches.
- [x] Oracle: `cargo xtask oracle locate` on a real dedicated server and a fixed case list; ignored comparison test (`crates/spyglass/tests/locate.rs`). The game's answer must be among our hits.
- [x] `spyglass-viewer` depends on `spyglass` only; update [architecture.md](../architecture.md#crates).
- [x] `docs/getting-started.md` and the README: using the CLI.
- [ ] Accept or revise ADRs 0014 and 0015.
- [ ] Milestone wrap-up: tick the exit criteria, move the m03 plans and the milestone to `completed/`, update the roadmap and plan indexes, fix links.

## Verification

Done on 2026-10-25 for 26.3:

- CLI integration tests: 4, every command in both output modes, plus argument errors.
- The `/locate` comparison: 300 cases (4 seeds × 3 positions × 25 arguments) all agree; the game's answer is our first hit in 286.
- The structure golden test still passes, now also checking that each of the 61 ring positions where the game differs is among our alternatives (all are).
- `cargo xtask check` passes with the new crates.

## Durable outputs

- [x] ADRs 0014 (public API) and 0015 (`/locate` on a real server)
- [x] `docs/specs/cli-json.md` and `docs/specs/locate.md`, listed in `docs/specs/README.md`
- [x] `docs/specs/oracle.md`: locate answers
- [x] `docs/architecture.md`: crates table and front ends
- [x] `docs/getting-started.md`, README: CLI usage

## Open questions

- ~~How far locate searches.~~ Resolved: the game's 100 rings by default, `--radius` to change it.
- ~~Candidates in locate answers.~~ Resolved: the nearest few in search order, labelled, up to the first certain one.

## Notes

- 2026-10-25: Plan drafted.
- 2026-10-25: Started. Disassembled the searches (above); both open questions answered.
- 2026-10-25: `findNearestMapStructure` needs a `ServerLevel`, so the plan's oracle subcommand became `cargo xtask oracle locate`, which drives the real dedicated server (ADR 0015). About 20 s per seed.
- 2026-10-25: The plan's `Session::new(bundle, seed, dimension)` became `World::open` plus `world.session(seed)`: the lower crates' seed-bound types borrow the loaded data, and a loaded world is reused across seeds (ADR 0014). Stronghold rings became lazy, since they take about 2.4 s and most queries don't need them.
- 2026-10-25: The first comparison failed one case: a stronghold whose ring position depends on tied biome cells. Flagging wasn't enough, because the game's position wasn't in the list. Ring positions now carry their alternative chunks, and locate lists them as candidates.
