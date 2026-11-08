# 0063. Threads within library queries: the current pool, sequential in a search

- Status: Accepted
- Date: 2026-11-07

## Context

Some single queries take seconds on one thread: `Session::nearest_feature` places chunks one at a time at about 30 ms each (2 s for `monster_room` on 26.3, 14 s on 26.2), `Session::features_in` does the same for a dungeon tile, and the first stronghold query settles 128 ring positions, each a biome search. Their parts are independent: ring positions draw from their own forks of the ring's random ([structure-placement.md](../specs/structure-placement.md#concentric-rings)), and a chunk's placing depends only on its neighbours' carved blocks. They can run in parallel and give the same answers, collected in the order they give now.

What makes it hard is that these queries are called from three kinds of thread:

- **The CLI's main thread**, with every core free.
- **A search's checks**, on the search's own `rayon` pool ([ADR 0021](0021-search-engine.md)), one seed per task, which already uses every thread it's given. While the viewer's map is busy, [ADR 0046](0046-the-viewers-search-yields-to-the-map.md)'s throttle holds all but one of them back, in a sleep outside rayon's scheduler.
- **The viewer's tile workers** and its locate thread: plain `std::thread`s, one fewer than the cores.

A `par_iter` runs on the pool it's called from. Inside a pool, nested work goes to that pool's queues, where any idle thread can take it. Outside every pool it goes to rayon's global pool, one thread per core by default, and the calling thread blocks until it's done.

`find` already runs its items in parallel, on a pool it builds from `SearchOptions::threads` and holds to the throttle. Ring positions are already settled one `OnceLock` per point in `Placements`.

## Decision

We will let library queries run their independent parts with rayon on whatever pool they're called from, and run them sequentially in a search's checks.

- **No new option.** A query called from inside a pool the caller installed uses that pool; called from anywhere else, it uses rayon's global pool. Its answer, and the order of its results, is the same as on one thread.
- **Sequential in a search.** A session the search binds for its checks runs its queries on the calling thread. Seeds are already the search's unit of parallel work, so nesting would add overhead without speed. Worse, nested work queued in the search's pool can be taken by threads that have no batch, ones the throttle never sees, so a held-back search would use more cores than its limit. The flag is crate-internal and reaches the lower crates as an argument, not a global.
- **`find`** keeps its own pool and throttle as it is.
- **What runs in parallel** is decided per query, by measurement, as the queries are changed. Each parallel query gets a test that one thread and many give the same answer. The specs say which parts are independent; this record says where they run.

## Consequences

- The CLI's long queries, and the viewer's locate, use every core with no change to their callers.
- A dungeon tile's work moves from its tile worker to the global pool while the worker waits. With several tiles pending, the global pool's threads work alongside the search's, as the tile workers do today, and ADR 0046's throttle still holds the search back because the tile queue still has jobs. Fewer tiles at once finish sooner, and a single tile no longer waits on one core.
- The viewer gains the global pool's threads, one per core, beside its tile workers, idle unless a query is running. Rayon's global pool is sized once per process: `RAYON_NUM_THREADS` changes it, and a library user can install their own pool around a call.
- `spyglass-structure` and `spyglass-feature`, or whichever crates hold the parallel parts, take `rayon` as a normal dependency, already in the tree and in `deny.toml`.
- A search checks queries as it does now, one thread per seed. Speedups inside a query don't reach the search, whose throughput comes from seeds.
- Each parallel query needs its caches (`Placements`' rings, `features.rs`' carved chunks, the per-seed `Mutex<HashMap>`s) safe to share and free of anything that can change an answer, such as tie-breaking that depends on history ([biome-lookup.md](../specs/biome-lookup.md#ties)).
- **No parallel work inside a once-cell's initializer, or under a lock.** A thread waiting on rayon work runs other jobs meanwhile, and one that reads the same `OnceLock` would find it half-filled by its own thread, which deadlocks or panics. Parallel work fills the parts first and the cell after, as `Placements::ring_positions` does.

## Alternatives considered

- **A thread count or pool in each query's options, as `find` has:** explicit, but every parallel query grows an option, each call or session builds or holds a pool, and every caller, the search included, has to set it.
- **Parallel only when the options ask:** safest, but the default stays slow, and the CLI and the viewer each have to know which queries to opt into.
- **Letting the search's checks nest freely:** no flag, but nested work slips past the throttle, and on a full pool it adds scheduling for no gain.
- **Plain threads per query:** no work stealing, and a thread per call made from inside a search would oversubscribe the machine.
