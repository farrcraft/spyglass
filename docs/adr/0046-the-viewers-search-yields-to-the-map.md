# 0046. The viewer's search yields its cores to the map

- Status: Proposed
- Date: 2026-10-29

## Context

The viewer runs a seed search and the map's tiles in one process. Each uses every core but one: the search on its own `rayon` pool ([ADR 0021](0021-search-engine.md)), the tiles on the viewer's worker threads. They run at the same priority, so while a search runs the map gets about half the machine. On the dev machine (8 cores, 16 threads, 26.3), filling a 1:4 window with biomes at the surface (48 tiles of 128 pixels on 15 workers) takes 8.8 s alone, 10.3 s beside a 4-thread search, 11.7 s beside 8 and 16.2 s beside 15. Panning or zooming during a long search is when the map is slowest.

A search can run for hours, so the map can't simply wait for it, and a search that stops whenever the map is busy makes no progress while someone pans. Both kinds of work are CPU-bound and share caches, so the cores have to be divided between them on purpose.

## Decision

We will let a search's caller limit, while it runs, how many of its threads work at once, and the viewer will limit its search to one thread while the map has tiles to compute.

- `spyglass-search` gets a `Throttle`, a shared limit like `Stop`. The runner takes a slot before each batch of 16 seeds and gives it back after, waiting in 2 ms sleeps while the limit is reached. A stopped search ignores the limit. `spyglass` exposes it as `SearchOptions::throttle` (`SearchThrottle`).
- The viewer sets the limit to one every frame its tile pool has jobs pending or running, and lifts it every frame it has none. One thread keeps the search going, and its progress moving, while the map fills.

Which seeds a search reports doesn't depend on the limit, as it doesn't on the thread count or timing ([ADR 0021](0021-search-engine.md)); a time-limited search checks fewer seeds.

## Consequences

- The map fills nearly as fast during a search as without one, and the search gets every core back once the map is idle.
- A search slows down whenever the map is busy, so a time-limited search covers fewer seeds while someone moves around the map.
- A new limit holds from each thread's next batch: up to 16 seeds' checks, a few milliseconds unconfirmed and tens of milliseconds confirmed.
- Held-back threads sleep inside the pool and wake every 2 ms to look again: a little CPU, and up to 2 ms before each gets back to work when the limit lifts.
- The public API has one more search option. Callers that don't set it see no change.

## Alternatives considered

- **Lower OS priority for the search's threads:** needs a platform API on each OS or a dependency, schedulers only weigh priorities, and a low-priority thread holding a lock the tiles want (the session's caches) would slow the map instead.
- **Fewer search threads always:** wastes the cores whenever the map is idle, which is most of a long search.
- **Pausing the search while tiles are pending:** no progress, and no progress shown, while panning.
- **Shrinking the tile pool during a search:** the map is what the user is looking at, so it's the work that should win.
