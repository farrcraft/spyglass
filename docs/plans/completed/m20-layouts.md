# m20. Piece layouts for the remaining structures

- Status: Completed
- Milestone: [20. Loot](../../roadmap/20-loot.md)
- Started: 2026-11-06
- Completed: 2026-11-06

## Goal

Lay out the pieces of the structures with loot that spyglass doesn't lay out yet: strongholds, nether fortresses, end cities, woodland mansions and ocean-ruin clusters. Then place their containers, as [m20-containers](m20-containers.md) does for the others. With them, the milestone's first exit criterion holds for every kind of structure with loot, not only the laid-out ones. Exact pieces also give these structures' bounds ([structure-placement.md](../../specs/structure-placement.md#bounds)), and let features near them be decided by their pieces rather than by everything within 8 chunks ([features.md](../../specs/features.md#structures)).

## Approach

**Layouts are ports of each structure's piece generator,** as mineshafts' were ([structure-placement.md](../../specs/structure-placement.md#mineshafts)): the start's context random `R`, the pieces in the game's order with their boxes, and the draws exactly as the game makes them. Each is checked against the structure golden files, which already record every valid start's piece boxes in order ([oracle.md](../../specs/oracle.md)), so no new oracle run is needed for the layouts. In order of size (26.3's classes):

- **Ocean-ruin clusters** (`OceanRuinPieces`, about 30 KiB): one template, or a large one with up to eight small ones around it, with random offsets, rotations and integrity. Templates from the bundle.
- **End cities** (`EndCityPieces`, about 36 KiB): towers, bridges, fat towers and the ship, generated recursively from templates, with the generation retried and pieces dropped where they collide. The End's terrain decides whether a start is valid; its heights are spyglass's already.
- **Woodland mansions** (`WoodlandMansionPieces`, about 58 KiB): a random floor plan on a grid (`MansionGrid`), rooms chosen per floor, then templates per cell.
- **Nether fortresses** (`NetherFortressPieces`, about 119 KiB): weighted lists of bridge and castle pieces, with counts and depth limits, built outward from the start, and pieces kept whose boxes don't collide. Some corridor pieces decide their chest at layout time.
- **Strongholds** (`StrongholdPieces`, about 131 KiB): weighted piece types with limits, the layout regenerated until it has a portal room, and the start's position from the concentric rings spyglass already places.

A spike reads each generator in 26.2, 26.3 and 26.4-snapshot-7 before porting it. Where the versions differ, the difference is a switch or a dispatch arm ([ADR 0026](../../adr/0026-how-game-versions-coexist.md)).

**Containers.** Once laid out, each structure's containers follow m20-containers' rules: the loot seed is a `nextLong()` from the chunk's structure random, after every earlier draw of the structure's pieces in that chunk. What draws before a chest differs by structure, and the spike lists it:

- **Template structures** (end cities, mansions, ocean ruins): the templates' loot seeds, then their data markers (end cities' `Chest`, mansions' chest markers, ocean ruins' `chest`), with processors drawing from the position, not that random. Data markers that spawn mobs (sentries, illagers, drowned) use other randoms, which the spike confirms.
- **Legacy pieces** (strongholds, fortresses): `postProcess` with random block selectors (strongholds' stone bricks draw per block) and the checks that read the world (a stronghold piece over liquid places nothing). The draw counts are ported per piece, over the blocks the chunk has when the piece places, and a piece whose reads spyglass can't decide makes its chunk's later containers candidates.

**Where it lives.** `spyglass-structure`, beside `mineshaft.rs`: one module a structure, each giving `Starts::assemble` its pieces. `Layout` and the bounds gain them, and the viewer's bounds with them.

## Tasks

- [x] **Spike:** each generator above in 26.2, 26.3 and 26.4-snapshot-7: its draws, its pieces' boxes, where the versions differ, and each piece's draws in `postProcess` up to its containers. Findings in the notes. _No generator differs between the versions; reconstructions in `target/spike/layouts/`._
- [x] **Ocean ruins:** the cluster layout; checked against the golden boxes. _`ocean_ruin.rs`: 198 starts, 1,318 pieces, the game's (26.3)._
- [x] **End cities:** the layout; checked against the golden boxes. _`end_city.rs`: 17 starts, 743 pieces, the game's in every version._
- [x] **Mansions:** the grid and its rooms; checked against the golden boxes. _`mansion.rs`. The golden files had none: both oracles now look further out for a set with no start near the origin. 10 starts, 5,670 pieces, the game's (26.3)._
- [x] **Fortresses:** the layout, with the chests decided at layout time; checked against the golden boxes. _`fortress.rs`: 144 starts, 16,710 pieces, the game's in every version._
- [x] **Strongholds:** the layout and its retries; checked against the golden boxes. _`stronghold.rs`. The structure golden files now try every ring position (format 3): 1,219 starts in 26.3 (189,891 pieces), 1,218 in 26.2, 1,271 in 26.4-snapshot-7, all the game's; rings placed differently on this platform are left to the ring comparison._
- [x] **Bounds:** the five structures' starts get bounds in `Layout`; structure-placement.md's Bounds section updated. _`Starts::assemble` gives them, and `start_bounds` uses them; nether fossils are the last without._
- [x] **Features near them:** structure doubt by these pieces' boxes, not by everything within 8 chunks; the change in candidates measured on the golden seeds. _Mineshafts too, which features.md said but the code didn't do. Feature calls compared (26.3): 26,898 before, 39,260 after; not compared for a structure's pieces 14,770 before, 2,408 after (mineshafts 14,224 to 1,726). No differences either way._
- [x] **Containers:** each structure's containers and seeds, certain or candidates, through m20-containers' oracle run and comparison. _No differences in any version; end cities 25 of 25 certain, fortresses 44 of 44 and mansions 54 of 54 in each; strongholds 48 of 86 in 26.2, 47 of 85 in 26.3, 49 of 85 in 26.4-snapshot-7; cold ruins 44, 44 and 45 of 49, warm 24, 24 and 25 of 28. Every container's contents the game's (2,483, 2,660 and 2,657). 26.4-snapshot-5 and -6 as snapshot-7, with 2,660 containers each._
- [x] **Cost:** a start's layout for each, timed; a locate shouldn't slow down for it. _`cargo xtask bench` (26.3): `structure_layout` takes 0.083 ms for a stronghold, 0.116 ms for a mansion, 0.031 ms for a fortress, 0.028 ms for an end city and 0.001 ms for an ocean ruin, against 9.8 ms for a village. A locate lays out only for bounds, and a confirmed mansion locate is 3.0 ms._
- [x] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan. _Both pass, `oracle-test` in every version._

## Verification

- Every start of these five structures in the structure golden files has the game's pieces, box for box in order, in every supported version.
- For the golden seeds' nearest start of each, every certain container is the game's, contents included, and every container the game placed is an answer.
- The share of feature candidates caused by these structures, before and after, recorded here.

## Durable outputs

- [x] structure-placement.md: a section for each layout, and the Bounds section.
- [x] features.md: structure doubt by their pieces.
- [x] loot.md: their containers.

## Notes

- 2026-11-06: **containers** (26.3, format 3 files): end cities 25 of 25 certain, fortresses 44 of 44, mansions 54 of 54, strongholds 47 of 85, ocean ruins 44 of 49 cold and 24 of 28 warm; no differences, and all 2,660 containers' contents the game's. Before mineshafts' own boxes counted for structure doubt, strongholds were 18 of 85 and ruins 22 and 18: most of their candidates read where a dungeon could have written, and those dungeons were candidates because a mineshaft's pieces could have been anywhere within 8 chunks. What's left: reads where a shipwreck's or a ruined portal's pieces could be (strongholds 24), dungeons (strongholds 11, ruins 7), ties. A ruin's chest after a doubtful read keeps its seed, but its height may be off; the comparison takes it at its x and z. Strongholds' shells read every edge block's air, so a stronghold's chest is certain only where all of them are.
- 2026-11-06: **oracle coverage.** No golden file had a stronghold (the rings start about 80 chunks out) or a mansion. The structure golden files (format 3) now try every ring position, and, for a set with no start in the regions, placement chunks outward until one starts; the container golden files (format 3) add the start at each ring set's nearest position and the nearest start of a set with none within 64 chunks. Strongholds keep their piece weights in statics, like fortresses and end cities, so the containers oracle makes Overworld starts one at a time too. A first try searched until each structure had two starts; that ran each seed to 6 minutes (18 abandoned-camp variants, rare igloos and outposts) and was cut back to sets with none.
- 2026-11-06: an ocean ruin's later pieces settle their height over what the start's earlier pieces placed in the chunk (cold ruins' three layers, overlapping cluster pieces), so the containers follow those pieces' blocks: kept by integrity, air and structure blocks dropped, waterlogged in water, read through a whole-template loader that keeps turning properties as they are. Two layers' chests at one spot are one container, the later seed, as the oracle records a structure's containers when it's done.
- 2026-11-06: **Spike** (26.2, 26.3, 26.4-snapshot-7; reconstructions in `target/spike/layouts/`). No generator differs in behaviour between the three versions: 26.3 takes `StructurePiecesBuilder` where 26.2 takes `StructurePieceAccessor`, 26.4-snapshot-7 makes lists with `new ArrayList`, and mansions and end cities gain 26.3's terrain-column test that `check.rs` already has. Templates differ only in `DataVersion` and 26.2's palette keys.
  - **End cities:** the rotation, then towers, bridges, fat towers and one ship, recursively, from templates. Collisions are tested only in `recursiveChildren`, against the list the subtree would join, by the first box hit, through `genDepth` stamps of `nextInt()`. No world reads, so the height only moves the layout. `shipCreated` is the generator's only static state, reset per start. Containers: `fat_tower_top` (2), `ship` (2), `third_floor_2` (1), each the template's draw then its `Chest` marker's, the seed; sentries and the elytra don't draw.
  - **Fortresses:** start at the chunk's (2, 64, 2), its direction `nextInt(4)`; pending children popped at `nextInt(size)`; depth up to 30, boxes with `minY > 10`; up to 5 weighted rolls, a factory's refusal falling through to the next entry; past 112 blocks a `BridgeEndFiller` is made and dropped, its constructor drawing `nextInt()` when its box is free; then `moveInsideHeights(48, 70)`. The left and right small corridor turns decide their chest at layout (`nextInt(3) == 0`); in `postProcess` the chests' seeds are the only draws. `placeCount` lives on static weight lists, reset per start.
  - **Strongholds:** each attempt reseeds with `setLargeFeatureSeed(seed + attempt, x, z)` until the portal room is placed; start at (2, 64, 2); 50 deep, 112 blocks; `moveBelowSeaLevel(sea, minY, R, 10)`. In `postProcess`, nearly every piece fills its box with the smooth-stone selector, drawing a `nextFloat()` for each edge block inside the chunk that isn't air, so most chests' seeds depend on caves and aquifers. Chests: chest corridor, room crossing type 2, library (two if tall).
  - **Mansions:** the rotation, the grid (`MansionGrid`: four corridor walks, rooms per floor with `Util.shuffle`), then the placer's walls, rooms and roofs, drawing only for room doors and templates. Pieces can be mirrored left-right, which `Template` doesn't do yet. `Chest*` markers place their chest at the marker, drawing once each, after the templates' own chests (up to 42 in a piece); mobs don't draw from the structure random.
  - **Ocean ruins:** nothing laid out yet. The rotation, `isLarge` (`nextFloat() <= 0.3f`), a warm template or a cold one in three layers (brick, cracked, mossy; integrity f, 0.7, 0.5), and for a large one a cluster on `nextFloat() <= 0.9f` of `nextInt(5) + 4` of 8 candidate spots. Boxes at y 90. Each chunk settles each piece's height from `OCEAN_FLOOR_WG` and a scan down the footprint; a later layer or overlapping piece scans the earlier ones' blocks. Chests at their `chest` markers, one draw each; no other draw.

- 2026-11-04: a plan of its own, rather than narrowing the milestone's first exit criterion to the structures spyglass already laid out. Its order assumes [m20-containers](m20-containers.md) is done first, so the containers here reuse its rules and oracle run.
