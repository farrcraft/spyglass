# m20. Loot containers

- Status: Draft
- Milestone: [20. Loot](../roadmap/20-loot.md)
- Started:
- Completed:

## Goal

Where each loot container is, with its loot table and loot seed, certain or a candidate, for the structures and features spyglass places. With [m20-tables](m20-tables.md)'s rolls, that's what's in the chests. It meets the first exit criterion: the golden seeds' nearest structures of each kind, their containers' contents the game's, in every supported version.

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

**What spyglass doesn't lay out has no containers yet.** Strongholds, fortresses, end cities, mansions and ocean-ruin clusters have no piece layout in spyglass ([structure-placement.md](../specs/structure-placement.md#bounds)). Their containers need their layouts first, each a port of its own size. The plan doesn't cover them, and the milestone's first exit criterion is narrowed to the kinds whose pieces spyglass lays out, unless the user decides otherwise (see the notes).

**Certainty.** As for features ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)), each container is certain or a candidate with a reason: an earlier draw spyglass doesn't know (a feature element, a piece it can't place), a block read it can't decide (a tie, a feature placed earlier, a neighbour decorated first), or the start itself being a candidate. A container in a chunk whose decoration spyglass can't settle has a position and table but no seed.

**The oracle.** The chunk oracle generates chunks without structures today ([ADR 0053](../adr/0053-the-oracle-decorates-chunks-on-their-own.md)). For containers it needs structure starts and references and their pieces placed during decoration. A new run, `cargo xtask oracle containers`, generates every chunk of the golden seeds' nearest start of each kind and records each loot container: position, block or entity, table and seed. A second run rolls them with the game's tables, giving contents, so the end-to-end comparison doesn't depend on m20-tables' own comparison. If adding structures to the chunk oracle is a design change, it gets an ADR.

## Tasks

- [ ] **Spike:** where each laid-out structure draws in `postProcess` and `placeInWorld`, in 26.2, 26.3 and 26.4-snapshot-7; every caller of `createChest`, `createDispenser`, `setLootTable` and `setBlockEntityLootTable`; feature elements in each jigsaw pool and where they fall in piece order. Findings in the notes.
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

- For the golden seeds' nearest start of each laid-out kind, in every supported version, every certain container is the game's, contents included, and every container the game placed is an answer.
- Each candidate has a reason, and the share of candidates per reason is recorded here.

## Durable outputs

- [ ] loot.md: containers, their seeds and certainty.
- [ ] oracle.md: `oracle containers`, structures in generated chunks.
- [ ] structure-placement.md: what each piece draws, where ported.
- [ ] An ADR if the chunk oracle's design changes.

## Notes

- 2026-11-04: **Open: structures with no layout.** The milestone asks for the nearest structure of each kind. Strongholds, fortresses, end cities and mansions have chests and no layout in spyglass. Either the exit criterion is narrowed to laid-out kinds, with the others in [Not yet planned](../roadmap/README.md#not-yet-planned), or a fourth plan ports their layouts. Decide before starting.
- 2026-11-04: from the survey that wrote this plan (26.3): templates with loot tables are in 7 jigsaw structures' pools: villages, outposts, bastions, ancient cities, ruined portals, trial chambers and abandoned camps. Trial chambers' vaults are rolled per player and stay out of scope; their decorated pots and dispensers have tables. Desert pyramids', trail ruins' and the well's suspicious blocks are archaeology, out of scope.
