# Decoration order around the spawn

The order the game decorates chunks in (runs their `FEATURES` step) when a world is created, and how much that order changes feature and loot answers. Background for a decision on a third certainty beside [ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)'s two. Read from the server jars of 26.2, 26.3 and 26.4-snapshot-8 on 2026-11-14 (disassembled locally with `javap`; nothing from the game is kept here). The three versions agree on everything below unless it says otherwise.

## What the game decorates at creation

### Before any player joins

A new world's startup (`MinecraftServer.createLevels`, then `prepareLevels`) asks for chunks in only one place: `setInitialSpawn`, which picks the world spawn.

- It reads the surface height at the centre of the generator's spawn origin chunk (`ChunkGenerator.getOrigin`), which loads that chunk in full.
- Then it walks a square spiral over the 11 × 11 chunks around the origin, at most 121 steps, calling `PlayerSpawnFinder.getSpawnPosInChunk` on each, and stops at the first chunk that has a spawn position. Each call loads its chunk in full.
- Every request is blocking: the main thread waits for the chunk before taking the next step of the spiral.
- With the bonus chest option, it also places the bonus chest feature at the spawn.

`prepareLevels` adds no spawn-area ticket: it only reactivates tickets saved with the world (`TicketStorage.activateAllDeactivatedTickets`), and a new world has none. There are no spawn chunks in these versions.

**How far a request decorates.** A chunk loaded in full needs, through the generation pyramid (`ChunkPyramid.GENERATION_PYRAMID`), its 3 × 3 at `INITIALIZE_LIGHT` (the `LIGHT` step's requirement), so its 3 × 3 is decorated. `FEATURES` in turn needs the 5 × 5 at `TERRAIN` (26.3 and 26.4; `CARVERS` in 26.2) and the 17 × 17 at `STRUCTURE_STARTS`, and writes blocks out to 1 chunk.

So **without a player, a new world decorates only the 3 × 3 chunks around each chunk the spiral visits**, plus the origin chunk's. If the spiral stops at its first chunk, that is the origin's 3 × 3, 48 blocks across, which is far smaller than the area the searches near the spawn ask about. How far the spiral goes is for the recordings to show.

### When the first player joins

A join adds tickets: `SPAWN_SEARCH` (`PlayerSpawnFinder.findSpawn` looks for a free position in the spawn radius, loading each candidate's chunk with `addTicketAndLoadWithRadius`), `PLAYER_SPAWN`, then `PLAYER_LOADING` and `PLAYER_SIMULATION` around the player, out to the view distance. This is what decorates the area around the spawn: at the default view distance of 10, chunks out to about 10 from the player reach `FULL`, and their neighbours are decorated with them.

The oracle's `locate` server has no player, so the recordings join one through the game's own `PrepareSpawnTask` on a stand-in connection ([ADR 0073](../adr/0073-the-oracle-joins-a-stand-in-player.md)).

### Order and concurrency

- **One step at a time.** All of a dimension's generation tasks run on that `ChunkMap`'s `worldgen` mailbox, a `ConsecutiveExecutor` over the background pool, so its tasks run one after another. `ChunkStatusTasks.generateFeatures` completes synchronously (`CompletableFuture.completedFuture`) on that mailbox. So two chunks of the same dimension are never decorated at the same time, and decoration is a total order. The recordings confirm it: no two `FEATURES` events overlap in any of them.
- **Where the order varies.** The steps before features run partly off the mailbox (terrain on the background pool), and a waiting generation task re-enters the mailbox through `ChunkTaskDispatcher`, ordered by its chunk's ticket level. So the order depends on when those earlier steps finish, which depends on threads and timing. At creation, the blocking spiral holds it to one requested chunk at a time, so only the order within each requested chunk's 3 × 3 is open.

### Recording it

- `net.minecraft.server.Main` takes `--jfrProfile`, which starts the game's JFR profiler (`JvmProfiler.start(Environment.SERVER)`) before the world loads, so startup is recorded.
- `ChunkStep.apply` emits `minecraft.ChunkGeneration` (`ChunkGenerationEvent`) for every step it runs: the chunk (`chunkPosX`, `chunkPosZ`), its block position, `status` (the status's id: `minecraft:features` in every version) and `level` (the dimension), with JFR's own start time, duration and thread. The duration ends when the step's future completes, which for `FEATURES` is before `apply` returns.
- The status names before `FEATURES` differ by version: 26.2 has `noise`, `surface` and `carvers`; 26.3 replaces them with `terrain`; 26.4-snapshot-8 adds `noise_biomes` before `biomes`.

## Measurements

Taken on 2026-11-14 with `cargo xtask oracle order`, `oracle replay` and `cargo xtask order-report` ([oracle.md](../specs/oracle.md#order-files)), on one Windows machine with 16 hardware threads, at view distance 10:

- 26.2 and 26.4-snapshot-8: the 10 golden seeds, twice each.
- 26.3: the 10 golden seeds twice at the default processor count and twice at 2 (`--cpus 2`), and 50 further seeds (`--random 50`) twice each.

### What's decorated

- **Startup** decorated exactly 9 chunks in every run of every seed and version: the spawn spiral stopped at its first chunk, so startup is the origin chunk's 3 × 3 and nothing more.
- **The join** decorated 559 to 729 chunks around the player's chunk (729 is a 27 × 27 square).

### How stable the order is

Two runs of a seed are compared over the chunks both decorated at the join. A pair of neighbouring chunks (8-connected) is "in opposite order" when the runs decorated them in opposite orders; a chunk has "other neighbours first" when the set of its neighbours decorated before it differs between the runs.

| Version             | Run pairs | Same spawn chunk | Neighbour pairs in opposite order | Chunks with other neighbours first |
| ------------------- | --------- | ---------------- | --------------------------------- | ---------------------------------- |
| 26.2                | 10        | 1/10             | 2,031/21,564 (9.4%)               | 3,007/5,746 (52.3%)                |
| 26.3, 16 processors | 60        | 21/60            | 12,705/152,415 (8.3%)             |                                    |
| 26.3, 2 processors  | 10        | 3/10             | 1,017/21,259 (4.8%)               |                                    |
| 26.3, both          | 70        | 24/70            | 13,970/177,314 (7.9%)             | 21,564/47,013 (45.9%)              |
| 26.4-snapshot-8     | 10        | 1/10             | 2,421/26,258 (9.2%)               | 3,693/6,955 (53.1%)                |

- **The spawn moves.** The join's spawn search picks a different position in the spawn radius each run, and the player's chunk differs between two runs about two times in three. The whole decorated area moves with it.
- **Within the area the runs share**, about 1 neighbouring pair in 11 is decorated in the opposite order, and about half of all chunks have a different set of neighbours decorated before them. Even seeds whose two runs joined in the same chunk are at 6 to 8%. Fewer workers make it more regular, but not stable: at 2 processors it's still 1 pair in 20.
- This is in line with Horizon's report of about 6% between two runs around a standing player ([prior art](prior-art.md)).

The order isn't stable between runs, so no model of the scheduler can say which order a given world was decorated in: a model can at best be as good as another run, and another run disagrees about half the chunks' neighbours.

### What the order changes

Each recorded order was replayed over shared chunks, and each decorated chunk's target calls compared with its calls decorated on its own ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)'s first answer, `tries`). "Between runs" compares two runs' replays over the chunks both decorated.

| Version         | Chunk runs | Differ from on its own | With a container | Chunks differing between runs |
| --------------- | ---------- | ---------------------- | ---------------- | ----------------------------- |
| 26.2            | 12,370     | 13 (0.11%)             | 10               | 3/5,746 (0.05%)               |
| 26.3            | 98,256     | 63 (0.06%)             | 51               | 27/71,523 (0.04%)             |
| 26.4-snapshot-8 | 14,499     | 18 (0.12%)             | 12               | 4/6,955 (0.06%)               |
| all             | 125,125    | 94 (0.08%, 1 in 1,330) | 73               | 34/84,224 (0.04%)             |

- **The replay checks out.** A chunk with nothing within 2 chunks decorated before it reproduces `tries`, and an order with its 8 neighbours first reproduces `after_neighbours` for 26.3's 4 golden chunks that neighbours change.
- **Where:** of the 94, 3 are within 2 chunks of the spawn chunk, 15 at 3 to 5, 47 at 6 to 10 and 29 further out. They're spread over the area, not gathered at the spawn.
- **Which features:** almost all are dungeons (`monster_room` 378 calls, `monster_room_deep` 160), then lava lakes (`lake_lava_underground` 30, `lake_lava_surface` 8) and one amethyst geode (4 calls). These are the features whose placement reads blocks a neighbour may already have written, as the golden files found ([features.md](../specs/features.md#certainty)).
- **Rarer than the baseline:** 1 chunk in 1,330 differs from on its own, against 1 in 700 with all 8 neighbours first. Most chunks have some neighbours decorated first at a join, but few neighbours reach far enough to matter.
- **Not reproducible:** chunks differ between two runs about half as often as they differ from on their own, so a chunk whose answer the order changes in one run often keeps its answer on its own in another. The game itself doesn't give one answer there.

### What follows

Around the spawn at creation, a chunk's answer on its own is the game's in about 99.9% of chunks. The rest depend on an order that changes from run to run, so a third answer, "as decorated when the world is created", would have to pick one run's order, and another run of the game disagrees in about half of the chunks it would change. [ADR 0074](../adr/0074-no-third-answer-for-the-order-at-creation.md) rejects it.
