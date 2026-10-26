# Locate queries

Status: stable. Covers `/locate structure` and `/locate biome` in the Overworld of the Default world preset, implemented in `spyglass-structure` and `spyglass-biome` and exposed by the `spyglass` crate ([ADR 0014](../adr/0014-public-api-world-and-session.md)). Checked against the real game's `/locate` ([ADR 0015](../adr/0015-locate-checks-on-a-real-server.md)) for 26.3.

`/locate` doesn't return the nearest structure or biome. It returns the first one its search order finds, so spyglass ports the search order and the distance measure exactly. Where spyglass can't decide whether the game generates a structure in a chunk (terrain, until milestone 07 on the [roadmap](../roadmap/README.md)), it returns every place the game's answer may be, in search order, each labelled `certain` or not.

## Text seeds

`WorldOptions.parseSeed`, as the world-creation screen and `server.properties` read a seed:

1. `String.trim()`: remove characters up to U+0020 from both ends.
2. Empty: a random seed. spyglass rejects it.
3. `Long.parseLong`: an optional `+` or `-`, then decimal digits, in range. spyglass rejects text that `parseLong` could read with non-ASCII Unicode digits rather than guess.
4. Otherwise `String.hashCode()` of the trimmed text (UTF-16 code units, `h = 31·h + c`, wrapping `int`), widened to `long`.

## Structures

`LocateCommand` calls `ChunkGenerator.findNearestMapStructure(level, holders, pos, 100, false)`, with `pos` the command source's block position.

**Targets.** The argument is a structure id or a `#tag`. A tag's entries are expanded in order, nested tags in place, duplicates kept at their first position; optional entries that don't exist are skipped. Structures that no set the dimension can generate are dropped. The targets are grouped by the sets that place them, in order of first appearance.

**"Generated" in a chunk.** `getStructureGeneratingAt` asks `StructureCheck`, which rejects a chunk if the placement's frequency restriction or `findValidGenerationPoint` fails, and otherwise generates the chunk's structure starts. So the game's answer is a chunk where its real choice ([structure-placement.md](structure-placement.md#choosing-a-structure)) is one of the targets, after the biome check, the exclusion zone and terrain validity. spyglass decides this per chunk from the selection:

- a **hit** if any try of the selection is a target;
- **certain** if the selection's last try passes for sure and every try is a target, so some target certainly starts;
- otherwise a **candidate**.

The position reported is `getLocatePos`: the chunk's minimum block plus the set's `locate_offset`.

**Search order.**

1. Concentric-ring sets (strongholds) first. The game takes the nearest ring position that generates, by `distSqr` from `pos` to the chunk's `(x·16 + 8, 32, z·16 + 8)`, keeping the first of equals. spyglass lists ring chunks nearest first, ending at the first certain one.
2. Random-spread sets share one loop over square rings `r = 0, 1, ..., radius` of placement cells around `pos`'s chunk. In ring `r`, each set scans `dx` from `-r` to `r` (outer) and `dz` from `-r` to `r` (inner), skipping cells not on the ring's edge, and takes the cell containing chunk `(chunkX + spacing·dx, chunkZ + spacing·dz)`. A set's answer in the ring is its **first** generated chunk in that order. The best answer overall is replaced only by a strictly nearer one (`distSqr` from `pos` to the locate position). The search ends after the first ring in which any set found something.

spyglass follows the same loop. Each set's list in a ring ends at its first certain hit, and the search ends after a ring that holds one.

**Ring positions that aren't certain.** A stronghold ring position can depend on tied biome cells or on the platform's `Math.sin`/`Math.cos` ([structure-placement.md](structure-placement.md#concentric-rings)). Each `RingPosition` carries `alternatives`: the chunks the game may use instead, from replaying the tied cells' outcomes and from settling the other value `Math.round` may give. Locate lists every alternative as a candidate. Where more than 12 tied cells make the alternatives unknown, the answer is marked incomplete.

**Answer.** `StructureLocate` has:

- `hits`, in the order above;
- `complete`: the search ended where the game's does (a ring with a certain hit, or the radius). Then the game's answer is one of the hits, or nothing if every hit is a candidate it rejects. Otherwise spyglass stopped after 32 candidates, or a ring position's alternatives are unknown;
- `certain_answer`: the game's answer if every candidate fails.

For most surface structures, every hit is a candidate until terrain is ported, so lists run to the 32-candidate cap. In practice the game's answer is usually the first hit; see [Testing](#testing).

**Distance shown.** `/locate` prints `Mth.floor(Mth.sqrt((float) (dx·dx + dz·dz)))` for structures, horizontal and in `int` arithmetic. spyglass reports the same as `blocks_away`.

## Biomes

`LocateCommand` calls `ServerLevel.findClosestBiome3d(predicate, pos, 6400, 32, 64)`, which is `BiomeSource.findClosestBiome3d`:

1. Nothing if no biome the source can produce matches.
2. Columns at `pos + 32·(dx, dz)` for `(dx, dz)` from `BlockPos.spiralAround(ZERO, 6400 / 32, EAST, SOUTH)`. That's the centre, then legs east, south, west, west, north, north, east × 3, and so on: leg `k` of `0..=4·200` yields `k / 2 + 1` positions, the first a step in the previous leg's direction. It covers the 401 × 401 square exactly and ends at (200, −200).
3. In each column, heights from `Mth.outFromOrigin(pos.y, minY + 1, maxY + 1, 64)`: `pos.y` clamped into range, then alternately above and below it, one step further each time, continuing one way once the other leaves the range. For the overworld from y 64: 64, 128, 0, 192, 256, 320.
4. The first `(x, y, z)` whose quart's biome matches is the answer.

A tied cell whose biomes disagree on the predicate is a candidate: the game's answer there depends on lookup history ([biome-lookup.md](biome-lookup.md#ties)). The list ends at the first certain match, or after 16 candidates. The distance shown is `Mth.floor(Mth.sqrt((float) pos.distSqr(result)))`, in 3D.

## Testing

- **Unit tests**: the spiral and `outFromOrigin` orders, text seeds, the shown distance, and the ring alternatives' rounding.
- **The structure golden test** checks that every ring position where the game differs from spyglass is flagged, and that the game's chunk is spyglass's or one of its alternatives. For 26.3: 61 of 1,280 differ, all with the game's chunk among known alternatives.
- **The `/locate` comparison** `crates/spyglass/tests/locate.rs` runs every case in `oracle-out/<version>/locate/` ([oracle.md](oracle.md#locate-answers)). The game's answer must be among spyglass's hits, unless spyglass's list stopped at the candidate cap first. If the game finds nothing, spyglass must have no certain hit. For 26.3: 300 cases (4 seeds, 3 positions, 18 structure arguments and 7 biomes) all agree, and in 286 the game's answer is spyglass's first hit. Run it with `cargo test --release -p spyglass --test locate -- --ignored --nocapture`.
