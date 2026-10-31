# Locate queries

Status: stable. Covers `/locate structure` and `/locate biome` in the Default world preset's three dimensions, implemented in `spyglass-structure` and `spyglass-biome` and exposed by the `spyglass` crate ([ADR 0014](../adr/0014-public-api-world-and-session.md)). Checked against the real game's `/locate` ([ADR 0015](../adr/0015-locate-checks-on-a-real-server.md)) for 26.3, 26.2 and the 26.4 snapshots; their differences are in _Versions_ notes. Targets a version doesn't have, such as 26.3's `#minecraft:abandoned_camp` in 26.2, are rejected by both.

`/locate` doesn't return the nearest structure or biome. It returns the first one its search order finds, so spyglass ports the search order and the distance measure exactly. Where spyglass can't decide whether the game generates a structure in a chunk, it returns every place the game's answer may be, in search order, each labelled `certain` or not. [Confirmed](#confirmed-mode), terrain decides nearly every chunk. For the structure that really is nearest, spyglass has a second search of its own: [nearest](#nearest).

## Text seeds

`WorldOptions.parseSeed`, as the world-creation screen and `server.properties` read a seed:

1. `String.trim()`: remove characters up to U+0020 from both ends.
2. Empty: a random seed. spyglass rejects it.
3. `Long.parseLong`: an optional `+` or `-`, then decimal digits, in range. spyglass rejects text that `parseLong` could read with non-ASCII Unicode digits rather than guess.
4. Otherwise `String.hashCode()` of the trimmed text (UTF-16 code units, `h = 31·h + c`, wrapping `int`), widened to `long`.

## Structures

`LocateCommand` calls `ChunkGenerator.findNearestMapStructure(level, holders, pos, 100, false)`, with `pos` the command source's block position.

**Targets.** The argument is a structure id or a `#tag`. The API resolves it once per world (`World::structure_query`, likewise `World::biome_query`), and sessions of that world reuse the result; an unknown id or tag is an error then, not at each search. A tag's entries are expanded in order, nested tags in place, duplicates kept at their first position; optional entries that don't exist are skipped. Structures that no set the dimension can generate are dropped. The targets are grouped by the sets that place them, in order of first appearance.

**"Generated" in a chunk.** `getStructureGeneratingAt` asks `StructureCheck`, which rejects a chunk if the placement's frequency restriction or `findValidGenerationPoint` fails, and otherwise generates the chunk's structure starts. So the game's answer is a chunk where its real choice ([structure-placement.md](structure-placement.md#choosing-a-structure)) is one of the targets, after the biome check, the exclusion zone and terrain validity. spyglass decides this per chunk from the selection:

- a **hit** if any try of the selection is a target;
- **certain** if the selection's last try passes for sure and every try is a target, so some target certainly starts;
- otherwise a **candidate**.

The position reported is `getLocatePos`: the chunk's minimum block plus the set's `locate_offset`.

The search handles only concentric rings and random spread. A target placed only by a [`dimension_origin`](structure-placement.md#dimension-origin) set is never found, though it generates, and a set of that type adds nothing to a mixed target's search.

_Versions:_ `ChunkGenerator.findNearestMapStructure` and its search are the same in 26.2, 26.3 and 26.4-snapshot-5, and so is `StructureCheck.canCreateStructure`. Whether a chunk generates a target follows each version's rules in [structure-placement.md](structure-placement.md), whose _Versions_ notes say where they differ.

**Search order.**

1. Concentric-ring sets (strongholds) first. The game takes the nearest ring position that generates, by `distSqr` from `pos` to the chunk's `(x·16 + 8, 32, z·16 + 8)`, keeping the first of equals. spyglass lists ring chunks nearest first, ending at the first certain one.
2. Random-spread sets share one loop over square rings `r = 0, 1, ..., radius` of placement cells around `pos`'s chunk. In ring `r`, each set scans `dx` from `-r` to `r` (outer) and `dz` from `-r` to `r` (inner), skipping cells not on the ring's edge, and takes the cell containing chunk `(chunkX + spacing·dx, chunkZ + spacing·dz)`. A set's answer in the ring is its **first** generated chunk in that order. The best answer overall is replaced only by a strictly nearer one (`distSqr` from `pos` to the locate position). The search ends after the first ring in which any set found something.

spyglass follows the same loop. Each set's list in a ring ends at its first certain hit, and the search ends after a ring that holds one.

**Ring positions that aren't certain.** A stronghold ring position can depend on tied biome cells or on the platform's `Math.sin`/`Math.cos` ([structure-placement.md](structure-placement.md#concentric-rings)). Each `RingPosition` carries `alternatives`: the chunks the game may use instead, from replaying the tied cells' outcomes and from settling the other value `Math.round` may give. Locate lists every alternative as a candidate. Where more than 12 tied cells make the alternatives unknown, the answer is marked incomplete.

**Answer.** `StructureLocate` has:

- `hits`, in the order above;
- `complete`: the search ended where the game's does (a ring with a certain hit, or the radius). Then the game's answer is one of the hits, or nothing if every hit is a candidate it rejects. Otherwise spyglass stopped after 32 candidates, or a ring position's alternatives are unknown;
- `certain_answer`: the game's answer if every candidate fails.

Unconfirmed, most surface structures' hits are all candidates, so lists run to the 32-candidate cap. In practice the game's answer is usually the first hit; see [Testing](#testing).

### Confirmed mode

`LocateOptions::confirm`, on by default, decides each chunk with the seed's terrain ([structure-placement.md](structure-placement.md#with-terrain)): the check runs at its real position, and terrain's own rejections apply. Hits are then candidates only behind a tied biome cell or a platform-dependent ring position. A list whose hits are all certain is the game's answer, at `certain_answer`. The first confirmed locate in a session binds the seed's terrain, and each candidate chunk costs a few height queries.

**Distance shown.** `/locate` prints `Mth.floor(Mth.sqrt((float) (dx·dx + dz·dz)))` for structures, horizontal and in `int` arithmetic. spyglass reports the same as `blocks_away`.

### Nearest

`/locate`'s answer is often not the nearest structure. A ring's first hit in scan order wins, even when another hit in the same ring is nearer. For seed 12345 in 26.3, `/locate minecraft:ancient_city` from the origin gives −288, −288, 407 blocks away: cells (−1, −1) and (0, −1) are both in ring 1, and `dx = −1` is scanned first. A city at 176, −176, 249 blocks away, generates too. In the golden locate cases, the nearest is nearer than `/locate`'s answer in about a third (96 of 276 for 26.3).

`Session::nearest_structure` (`Starts::nearest` in `spyglass-structure`) is spyglass's own search, not the game's. It answers with the nearest start instead:

- **Distance:** horizontal, from `pos` to the start's locate position, as `dx² + dz²` in 128-bit integers. That's the distance `/locate` prints, not the 3D `distSqr` its search compares.
- **Ties**, at equal distances: the set that comes first among the targets' sets (in `/locate`'s order of first appearance), then the lower chunk z, then the lower chunk x. Where `/locate`'s scan order picks another start at the same distance, the two answers differ only by this rule.
- **What's searched:** the same cells `/locate` searches with the same radius, per set. That's every concentric-ring position with its alternatives, and random-spread rings `0..=radius` around the cell of `pos`'s chunk, the cell holding chunk `(chunkX + spacing·dx, chunkZ + spacing·dz)` as above. The answer is the nearest start in those cells.
- **Order of work:** ring positions first, then random-spread rings outward. In each batch, the chunks that pass the placement test (cheap: no terrain) are sorted by distance and the tie order. They're decided nearest first, only while no certain start is nearer, so terrain is asked about no more chunks than the answer needs. After ring `r`, the search stops if every cell of ring `r + 1` is farther than the best certain start. A cell's distance is from `pos` to the box its locate positions span: chunks `cell·spacing` to `cell·spacing + spacing − separation − 1` on each axis, times 16, plus the offset. A cell whose box would wrap in `int` arithmetic counts as distance 0.
- **Answer:** `hits` are nearest first: every candidate nearer than the nearest certain start, then that start, at `certain_answer`. With no certain start, every candidate is listed. Candidates are decided as for `/locate` (confirmed or not), and ring alternatives and tie- or platform-dependent positions are candidates. `complete` is false if more than 32 candidates were nearer than any certain start, or a ring position's alternatives are unknown, as for `/locate`. Otherwise the nearest start that generates is one of the hits.

Biome locate has no nearest mode. `/locate biome` already searches outward, column by column, though in a square spiral rather than by distance.

The CLI's `locate structure --nearest` and the viewer's Locate tab use it. The viewer shows both answers when they differ.

## Biomes

The argument is a biome id or a `#tag`, as `/locate biome` takes them. `World::biome_query` expands a tag's entries as structure tags are: nested tags in place, duplicates once, optional entries that don't exist skipped, a missing required entry or tag an error, and a tag that includes itself an error. The predicate is whether a biome is one of them.

`LocateCommand` calls `ServerLevel.findClosestBiome3d(predicate, pos, 6400, 32, 64)`, which is `BiomeSource.findClosestBiome3d`:

1. Nothing if no biome the source can produce matches.
2. Columns at `pos + 32·(dx, dz)` for `(dx, dz)` from `BlockPos.spiralAround(ZERO, 6400 / 32, EAST, SOUTH)`. That's the centre, then legs east, south, west, west, north, north, east × 3, and so on: leg `k` of `0..=4·200` yields `k / 2 + 1` positions, the first a step in the previous leg's direction. It covers the 401 × 401 square exactly and ends at (200, −200).
3. In each column, heights from `Mth.outFromOrigin(pos.y, minY + 1, maxY + 1, 64)`: `pos.y` clamped into range, then alternately above and below it, one step further each time, continuing one way once the other leaves the range. For the overworld from y 64: 64, 128, 0, 192, 256, 320.
4. The first `(x, y, z)` whose quart's biome matches is the answer.

A tied cell whose biomes disagree on the predicate is a candidate: the game's answer there depends on lookup history ([biome-lookup.md](biome-lookup.md#ties)). The list ends at the first certain match, or after 16 candidates. The distance shown is `Mth.floor(Mth.sqrt((float) pos.distSqr(result)))`, in 3D.

_Versions:_ the search is the same in all three. 26.2's `BiomeSource.findClosestBiome3d` takes a `Climate.Sampler` and reads `getNoiseBiome(qx, qy, qz, sampler)`; 26.3 and 26.4-snapshot-5 take the `RandomState` and read through `createCachingResolver(randomState)`. Every unique cell's biome is the same, and a tied cell stays a candidate. In 26.4-snapshot-5 chunks store a biome per block ([world-saves.md](world-saves.md#chunk-biomes)), but `/locate biome` still tests quart biomes from the biome source, not stored ones.

## Limits

The game's own searches have fixed sizes. The API takes others, up to limits that keep any one request to at most minutes of work and a bounded amount of memory:

| Request                      | Game's size  | Most allowed                              |
| ---------------------------- | ------------ | ----------------------------------------- |
| `locate_structure`'s radius  | 100 rings    | 1,000 rings (`MAX_LOCATE_RADIUS`)         |
| `locate_biome`'s radius      | 6,400 blocks | 25,600 blocks (`MAX_LOCATE_BIOME_RADIUS`) |
| `slime_chunks_near`'s radius | —            | 1,000 chunks (`MAX_SLIME_RADIUS`)         |
| `heights_in`'s region        | —            | 65,536 columns (`MAX_HEIGHT_COLUMNS`)     |

More is `Error::TooLarge`, naming what was asked and the limit. Region sizes are computed in 64-bit integers, saturating, so no region is too large to report.

## Testing

- **Unit tests**: the spiral and `outFromOrigin` orders, text seeds, the shown distance, and the ring alternatives' rounding.
- **The structure golden test** checks that every ring position where the game differs from spyglass is flagged, and that the game's chunk is spyglass's or one of its alternatives. For 26.3: 61 of 1,280 differ, all with the game's chunk among known alternatives.
- **Nearest**, on fixture bundles: for origins across the fixture's plains and desert and at the world border, confirmed and not, the answer is the nearest certain start among everything `structures_in` finds around it. Every nearer candidate is listed, and hits are sorted by distance. The tests cover ring sets, mixed tags, the candidate cap, and that the answer is never farther than `/locate`'s. On the real bundles, an ignored test in `crates/spyglass/tests/locate.rs` checks every golden structure case of each version the same way, confirmed: no certain start is nearer than the answer unless it's listed as a candidate, and the game's own answer is never nearer. For 26.3, the answer is certain in all 276 cases. In 4, candidates come first: strongholds behind tie- or platform-dependent ring positions.
- **The `/locate` comparison** `crates/spyglass/tests/locate.rs` runs every case in `oracle-out/<version>/locate/` ([oracle.md](oracle.md#locate-answers)). The game's answer must be among spyglass's hits, unless spyglass's list stopped at the candidate cap first. If the game finds nothing, spyglass must have no certain hit. Every case is run in its dimension. For 26.3: 480 cases (4 seeds; 3 positions per dimension; the Overworld's 18 structure arguments and 7 biomes, the Nether's 4 and 5, the End's 1 and 5) all agree, and in 452 the game's answer is spyglass's first hit. Every structure case is run again confirmed: where all of spyglass's hits are certain, its certain answer must be exactly the game's (or nothing where the game finds nothing). For 26.3 that's 272 of the 276 structure cases, all equal: every one of the Nether's 48 and the End's 12, and 212 of the Overworld's 216; the other 4 are strongholds behind tie- or platform-dependent ring positions. Run it with `cargo test --release -p spyglass --test locate -- --ignored --nocapture`.
