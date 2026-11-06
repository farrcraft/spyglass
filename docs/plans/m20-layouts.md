# m20. Piece layouts for the remaining structures

- Status: Draft
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started:
- Completed:

## Goal

Lay out the pieces of the structures with loot that spyglass doesn't lay out yet: strongholds, nether fortresses, end cities, woodland mansions and ocean-ruin clusters. Then place their containers, as [m20-containers](completed/m20-containers.md) does for the others. With them, the milestone's first exit criterion holds for every kind of structure with loot, not only the laid-out ones. Exact pieces also give these structures' bounds ([structure-placement.md](../specs/structure-placement.md#bounds)), and let features near them be decided by their pieces rather than by everything within 8 chunks ([features.md](../specs/features.md#structures)).

## Approach

**Layouts are ports of each structure's piece generator,** as mineshafts' were ([structure-placement.md](../specs/structure-placement.md#mineshafts)): the start's context random `R`, the pieces in the game's order with their boxes, and the draws exactly as the game makes them. Each is checked against the structure golden files, which already record every valid start's piece boxes in order ([oracle.md](../specs/oracle.md)), so no new oracle run is needed for the layouts. In order of size (26.3's classes):

- **Ocean-ruin clusters** (`OceanRuinPieces`, about 30 KiB): one template, or a large one with up to eight small ones around it, with random offsets, rotations and integrity. Templates from the bundle.
- **End cities** (`EndCityPieces`, about 36 KiB): towers, bridges, fat towers and the ship, generated recursively from templates, with the generation retried and pieces dropped where they collide. The End's terrain decides whether a start is valid; its heights are spyglass's already.
- **Woodland mansions** (`WoodlandMansionPieces`, about 58 KiB): a random floor plan on a grid (`MansionGrid`), rooms chosen per floor, then templates per cell.
- **Nether fortresses** (`NetherFortressPieces`, about 119 KiB): weighted lists of bridge and castle pieces, with counts and depth limits, built outward from the start, and pieces kept whose boxes don't collide. Some corridor pieces decide their chest at layout time.
- **Strongholds** (`StrongholdPieces`, about 131 KiB): weighted piece types with limits, the layout regenerated until it has a portal room, and the start's position from the concentric rings spyglass already places.

A spike reads each generator in 26.2, 26.3 and 26.4-snapshot-7 before porting it. Where the versions differ, the difference is a switch or a dispatch arm ([ADR 0026](../adr/0026-how-game-versions-coexist.md)).

**Containers.** Once laid out, each structure's containers follow m20-containers' rules: the loot seed is a `nextLong()` from the chunk's structure random, after every earlier draw of the structure's pieces in that chunk. What draws before a chest differs by structure, and the spike lists it:

- **Template structures** (end cities, mansions, ocean ruins): the templates' loot seeds, then their data markers (end cities' `Chest`, mansions' chest markers, ocean ruins' `chest`), with processors drawing from the position, not that random. Data markers that spawn mobs (sentries, illagers, drowned) use other randoms, which the spike confirms.
- **Legacy pieces** (strongholds, fortresses): `postProcess` with random block selectors (strongholds' stone bricks draw per block) and the checks that read the world (a stronghold piece over liquid places nothing). The draw counts are ported per piece, over the blocks the chunk has when the piece places, and a piece whose reads spyglass can't decide makes its chunk's later containers candidates.

**Where it lives.** `spyglass-structure`, beside `mineshaft.rs`: one module a structure, each giving `Starts::assemble` its pieces. `Layout` and the bounds gain them, and the viewer's bounds with them.

## Tasks

- [ ] **Spike:** each generator above in 26.2, 26.3 and 26.4-snapshot-7: its draws, its pieces' boxes, where the versions differ, and each piece's draws in `postProcess` up to its containers. Findings in the notes.
- [ ] **Ocean ruins:** the cluster layout; checked against the golden boxes.
- [ ] **End cities:** the layout; checked against the golden boxes.
- [ ] **Mansions:** the grid and its rooms; checked against the golden boxes.
- [ ] **Fortresses:** the layout, with the chests decided at layout time; checked against the golden boxes.
- [ ] **Strongholds:** the layout and its retries; checked against the golden boxes.
- [ ] **Bounds:** the five structures' starts get bounds in `Layout`; structure-placement.md's Bounds section updated.
- [ ] **Features near them:** structure doubt by these pieces' boxes, not by everything within 8 chunks; the change in candidates measured on the golden seeds.
- [ ] **Containers:** each structure's containers and seeds, certain or candidates, through m20-containers' oracle run and comparison.
- [ ] **Cost:** a start's layout for each, timed; a locate shouldn't slow down for it.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan.

## Verification

- Every start of these five structures in the structure golden files has the game's pieces, box for box in order, in every supported version.
- For the golden seeds' nearest start of each, every certain container is the game's, contents included, and every container the game placed is an answer.
- The share of feature candidates caused by these structures, before and after, recorded here.

## Durable outputs

- [ ] structure-placement.md: a section for each layout, and the Bounds section.
- [ ] features.md: structure doubt by their pieces.
- [ ] loot.md: their containers.

## Notes

- 2026-11-04: a plan of its own, rather than narrowing the milestone's first exit criterion to the structures spyglass already laid out. Its order assumes [m20-containers](completed/m20-containers.md) is done first, so the containers here reuse its rules and oracle run.
