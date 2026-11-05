# m20. Loot containers

- Status: In progress
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started: 2026-11-04
- Completed:

## Goal

Where each loot container is, with its loot table and loot seed, certain or a candidate, for the structures and features spyglass places. With [m20-tables](completed/m20-tables.md)'s rolls, that's what's in the chests. It meets the first exit criterion for the structures spyglass lays out and for dungeons: the golden seeds' nearest of each kind, their containers' contents the game's, in every supported version. [m20-layouts](m20-layouts.md) adds the rest.

## Approach

**Where a loot seed comes from.** Every container's seed is a `nextLong()` from the random that places it, drawn when its block entity exists (26.3 bytecode):

- **Features:** `RandomizableContainer.setBlockEntityLootTable(level, random, pos, table)` with the feature's random. Dungeons (`monster_room`) already draw it in placing ([features.md](../specs/features.md#monster-room)); the try just needs to keep the position, the table and the seed. The bonus chest is a feature only in new worlds with the option, and out of scope.
- **Templates:** `StructureTemplate.placeInWorld` puts `LootTableSeed` from its random parameter into each block's NBT whose block entity is a `RandomizableContainer`, in the template's block order, for blocks inside the box being placed. A jigsaw piece (`SinglePoolElement.place`) passes the chunk's structure random; its processors use a random from the position (`Mth.getSeed`), never that one. So within a jigsaw piece, the loot seeds are that random's only draws.
- **Legacy pieces:** `StructurePiece.createChest` and `createDispenser`, inside the box and not already a chest, set the block and draw. Mineshaft corridors place a minecart with a chest (`MinecartChest.setLootTable(table, random.nextLong())`) where the floor holds. Shipwrecks, ocean ruins, igloos, end cities and mansions place templates and then set chests at data markers with the same random.

**The structure random.** Decorating a chunk, each step places its structures before its features: for each structure of the step in the registry's order, the decoration random is reseeded with `setFeatureSeed(decorationSeed, index, step)` and every start of that structure reaching the chunk places its pieces in the chunk, in piece order, all from that one random ([features.md](../specs/features.md#structures)). A container's seed is certain when every earlier draw from that random in that chunk is: the earlier containers, and whatever else the pieces draw.

**What else the pieces draw**, by structure, to be confirmed in the spike:

- **Jigsaw** (villages, outposts, bastions, ancient cities, trail ruins, trial chambers, and 26.3's abandoned camps): only the loot seeds, except `feature_pool_element`s, whose features (village trees, hay piles and the like) draw from the same random. A container after a feature spyglass doesn't place is a candidate. The spike counts them: if villages' trees decide most, placing those features is a task.
- **Single-piece structures:** the buried treasure's chest is its only draw. The desert pyramid's four chests and the jungle temple's chests and dispensers come after block selectors that draw per block (the temple's mossy stone). Their draw counts don't depend on the world, so they're ported in full.
- **Template structures with data markers** (shipwrecks, igloos, ocean ruins, ruined portals): the template's draws, then the markers'.
- **Mineshafts:** corridors draw per block for cobwebs, rails and supports, and read the blocks (`isInInvalidLocation`, air below a minecart). Their containers need each corridor's `postProcess` ported over carved blocks. They're the most common container underground, so this is a task of its own, measured first.

**Structures spyglass doesn't lay out** (strongholds, fortresses, end cities, mansions and ocean-ruin clusters, [structure-placement.md](../specs/structure-placement.md#bounds)) get their layouts and containers in [m20-layouts](m20-layouts.md), after this plan, with its rules.

**Certainty.** As for features ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)), each container is certain or a candidate with a reason: an earlier draw spyglass doesn't know (a feature element, a piece it can't place), a block read it can't decide (a tie, a feature placed earlier, a neighbour decorated first), or the start itself being a candidate. A container in a chunk whose decoration spyglass can't settle has a position and table but no seed.

**The oracle.** The chunk oracle generates chunks without structures today ([ADR 0053](../adr/0053-the-oracle-decorates-chunks-on-their-own.md)). For containers it needs structure starts and references and their pieces placed during decoration. A new run, `cargo xtask oracle containers`, generates every chunk of the golden seeds' nearest start of each kind and records each loot container: position, block or entity, table and seed. A second run rolls them with the game's tables, giving contents, so the end-to-end comparison doesn't depend on m20-tables' own comparison. If adding structures to the chunk oracle is a design change, it gets an ADR.

## Tasks

- [x] **Spike:** where each laid-out structure draws in `postProcess` and `placeInWorld`, in 26.2, 26.3 and 26.4-snapshot-7; every caller of `createChest`, `createDispenser`, `setLootTable` and `setBlockEntityLootTable`; feature elements in each jigsaw pool and where they fall in piece order. Findings in the notes. _Every structure's draws are the same in 26.2, 26.3 and 26.4-snapshot-7; reconstructions in `target/spike/containers/`._
- [ ] **Dungeons:** each placed dungeon's chests: position, table and seed.
- [ ] **Jigsaw containers:** the template blocks with loot tables, their order within a piece and across pieces, clipped to the chunk; certainty by earlier feature elements.
- [ ] **Single-piece structures:** buried treasure, desert pyramid, jungle temple (chests and dispensers), igloo, shipwreck, ruined portal: each one's draws up to its containers.
- [ ] **Mineshafts:** measure how many golden containers are minecart chests; port the corridors' `postProcess` over the carved blocks.
- [ ] **Maps:** the exploration map's target from locate, from the container, for a fresh world.
- [ ] **Oracle:** structures in generated chunks; `oracle containers` and its golden file; the CI unit.
- [ ] **Comparison:** every certain container's position, table, seed and contents are the game's; every game's container is an answer, certain or a candidate.
- [ ] **Spec:** loot.md's containers section, and features.md's monster room.
- [ ] `cargo xtask check` and `cargo xtask oracle-test`. Complete the plan.

## Verification

- For the golden seeds' nearest start of each kind spyglass lays out, in every supported version, every certain container is the game's, contents included, and every container the game placed is an answer.
- Each candidate has a reason, and the share of candidates per reason is recorded here.

## Durable outputs

- [ ] loot.md: containers, their seeds and certainty.
- [ ] oracle.md: `oracle containers`, structures in generated chunks.
- [ ] structure-placement.md: what each piece draws, where ported.
- [ ] An ADR if the chunk oracle's design changes.

## Notes

- 2026-11-04: **Spike: the structure random** (26.2, 26.3, 26.4-snapshot-7, the same in all three).
  - **Seeding:** each step's structures in registry order, which is by file name; `setFeatureSeed(decorationSeed, index, step)` with `index` counting every structure of the step, near or not. `underground_structures`: buried treasure 0, mineshaft 1, mesa mineshaft 2, trail ruins 3, trial chambers 4. In `surface_structures` 26.3's 18 abandoned camps take 0 to 17 and move the rest up 18: data, not a switch.
  - **Starts** reaching the chunk come in the order of its reference set (a fastutil `LongOpenHashSet`), all from one random. Each places its pieces whose boxes meet the chunk (x0..x0+15, z0..z0+15, `minY + 1` up), in assembly order, then `afterPlace`.
  - **Templates** draw from the chunk random only for loot seeds: no structure piece calls `setRandom`, so the palette and every processor take `RandomSource.create(Mth.getSeed(pos))`. A seed is `nextLong()` for each block with NBT inside the chunk whose block entity is a `RandomizableContainer`, whether or not it has a table, in template (y, x, z) order whatever the rotation. A block a processor drops doesn't draw. Containers: chests (copper ones too), trapped chests, barrels, dispensers, droppers, hoppers, shulker boxes, crafters, decorated pots; not furnaces, brewing stands, vaults or suspicious blocks.
  - **`TemplateStructurePiece`:** the template, then its data markers in (y, x, z) order, each with the same random, then the jigsaw blocks (no draws).
  - **Feature elements** (`feature_pool_element`) place a whole placed feature on the chunk random. Villages (trees, hay, melons, ice, snow, berry bushes), abandoned camps (trees) and ancient cities (sculk patches) have them, and one can come before a container in the same chunk; bastions, outposts, trial chambers, trail ruins and ruined portals have none.
- 2026-11-04: **Spike: what each structure draws** before its containers:
  - **Certain from the layout:** desert pyramid (each chunk's first draw `nextInt(3)`; its four chests draws 2 to 5 of the start chunk, N, E, S, W); jungle temple (1,511 `nextFloat`s of its moss selector, then the two dispensers, the chest, 11 more, the hidden chest); igloo (the bottom's NBT chest draws once, then its `chest` marker sets air and draws the seed); ocean ruins (a `chest` marker each, no template containers); ocean shipwrecks (k template draws, then k marker draws, the seeds); ruined portals (one chest, the first draw in the chunk holding the template's centre, placed unclipped). Jigsaw containers before any feature element in their chunk.
  - **Certain, position from blocks:** buried treasure, the chunk's first draw; its y from `OCEAN_FLOOR_WG` and a scan down to stone-like blocks, after carvers.
  - **Candidates:** beached shipwrecks (one `nextInt(3)` belongs to the first chunk decorated, which the seed can't tell); containers after a feature element; mineshaft minecarts, whose corridors' draws depend on the blocks around them (supports on non-air ceilings, cobwebs below `OCEAN_FLOOR_WG`, a minecart on air over a non-air block, the whole corridor skipped beside liquid); a chest where `protected_blocks` meets an earlier feature's block.
  - **Census (26.3):** villages 80 containers in 67 of 478 templates (18 fisher barrels without a table); outposts 2; bastions 37; ancient cities 15; trial chambers 29 (chests, dispensers, pots, barrels, a hopper); trail ruins none; abandoned camps 414 in all 297 templates.
  - **Piece state is decided by the first chunk decorated:** a pyramid's height, a wreck's height and its beached draw, a spider corridor's spawner. Only the beached draw changes a seed.

- 2026-11-04: structures with no layout get [m20-layouts](m20-layouts.md), a plan of their own, rather than a narrower exit criterion.
- 2026-11-04: from the survey that wrote this plan (26.3): templates with loot tables are in 7 jigsaw structures' pools: villages, outposts, bastions, ancient cities, ruined portals, trial chambers and abandoned camps. Trial chambers' vaults are rolled per player and stay out of scope; their decorated pots and dispensers have tables. Desert pyramids', trail ruins' and the well's suspicious blocks are archaeology, out of scope.
