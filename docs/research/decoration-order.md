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

The oracle's server has no player. Measuring the order at a join needs a joined player, from a real client or the game's own player list over a stand-in connection.

### Order and concurrency

- **One step at a time.** All of a dimension's generation tasks run on that `ChunkMap`'s `worldgen` mailbox, a `ConsecutiveExecutor` over the background pool, so its tasks run one after another. `ChunkStatusTasks.generateFeatures` completes synchronously (`CompletableFuture.completedFuture`) on that mailbox. So two chunks of the same dimension are never decorated at the same time, and decoration is a total order. The recordings should confirm this: their `FEATURES` events shouldn't overlap.
- **Where the order varies.** The steps before features run partly off the mailbox (terrain on the background pool), and a waiting generation task re-enters the mailbox through `ChunkTaskDispatcher`, ordered by its chunk's ticket level. So the order depends on when those earlier steps finish, which depends on threads and timing. At creation, the blocking spiral holds it to one requested chunk at a time, so only the order within each requested chunk's 3 × 3 is open.

### Recording it

- `net.minecraft.server.Main` takes `--jfrProfile`, which starts the game's JFR profiler (`JvmProfiler.start(Environment.SERVER)`) before the world loads, so startup is recorded.
- `ChunkStep.apply` emits `minecraft.ChunkGeneration` (`ChunkGenerationEvent`) for every step it runs: the chunk (`chunkPosX`, `chunkPosZ`), its block position, `status` (the status's id: `minecraft:features` in every version) and `level` (the dimension), with JFR's own start time, duration and thread. The duration ends when the step's future completes, which for `FEATURES` is before `apply` returns.
- The status names before `FEATURES` differ by version: 26.2 has `noise`, `surface` and `carvers`; 26.3 replaces them with `terrain`; 26.4-snapshot-8 adds `noise_biomes` before `biomes`.

## Measurements

Not taken yet.
