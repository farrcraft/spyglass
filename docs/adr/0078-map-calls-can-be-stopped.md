# 0078. Map calls can be stopped

- Status: Proposed
- Date: 2026-11-16

## Context

The viewer computes its map in tiles on a pool of worker threads, one tile per job, through the `spyglass` crate's map calls: `biome_map`, `height_map`, `surface_biome_map`, `carving_map`, `underground_map`, `flower_map`, `map_features` (structures, features and loot), `containers_in` and `containers_holding`. Some jobs take seconds:

- a features or loot tile at 1:2 places hundreds of chunks, about 30 ms each;
- a height tile at 1:64 takes about 20 s ([render.md](../specs/render.md#heights));
- an underground tile with carvers at 1:4 takes over 2 s.

When the user zooms out, pans away or switches a layer off, the jobs no longer wanted are dropped before they start, but a running job can't be interrupted. So the workers stay busy for seconds on tiles nobody will see, while the tiles that are wanted wait.

Searches already solve this: `spyglass_search::Stop` (re-exported as `spyglass::SearchStop`) is an atomic flag the caller raises from another thread, checked between batches of seeds. The map calls take no such signal.

Three things make it harder than adding a parameter:

- **The slow loops aren't all in `spyglass`.** Biome grids are filled by `spyglass-biome` and terrain heights by `spyglass-terrain`. `Stop` lives in `spyglass-search`, the top of the crate order, and lower crates must not depend on higher ones.
- **Splitting work must not change answers.** Heights are sampled chunk by chunk, each chunk's density volume sized to the columns it samples, which gives each column the answer it would have alone ([render.md](../specs/render.md#heights)). Any way of stopping part-way must keep that.
- **The public API is used by three front ends** (the viewer, the CLI, the oracle comparisons), and only the viewer needs to stop a map call.

## Decision

We will let a caller stop a map call through a view of the session. `Session::stopping(&self, stop: &Stop) -> Stopping<'_, 'w>` returns a borrowed view with the same map calls as `Session`. A call made through it checks the stop as it goes, and returns `Error::Stopped` soon after the stop is raised. Calls made on `Session` itself are unchanged and never stop.

- **One stop type.** `spyglass_search::Stop` is re-exported as `spyglass::Stop`. `SearchStop` stays one release as a deprecated alias.
- **Checks only in the `spyglass` crate.** Each call checks the stop where it already loops, or in loops it gains for this:
  - before each chunk it places (features, containers) or carves, including the neighbours a placed chunk carves, since the first chunks a call places carve all of theirs;
  - before each candidate structure start, which confirming with terrain makes slow;
  - between bands of rows, where a lower crate fills a whole map (heights, the underground probes). A band is the rows over one row of chunks, or one row of pixels where pixels are a chunk or more apart. The terrain samples each chunk's columns together whatever else it's asked for, so a band at a time gives the same answers;
  - before each row, in the surface biome and flower maps;
  - once before it starts, for a biome map, which takes well under a tenth of a second a tile.

  Lower crates are unchanged.

- **Prompt enough, not instant.** On the 26.3 Overworld in a release build, the slowest tiles (heights at 1:64, every underground layer at 1:4, features and every container at 1:2) stopped 11 to 56 ms after the stop was raised.
- **A world's first-use loads don't stop.** Loading the jigsaw template pools, which features and containers read structure pieces from, took 4.4 s the first time on 26.3 with every core waiting for it. It happens once per world and every later call shares it, so it isn't interrupted.
- **`structure_layout` doesn't stop.** A layout takes milliseconds.
- **A stopped call returns nothing.** It returns no partial map. `Error` is already `#[non_exhaustive]` ([ADR 0077](0077-open-ended-public-enums-are-non-exhaustive.md)), so `Error::Stopped` breaks no caller.

## Consequences

- The viewer can stop a running tile job within half a second of not wanting it, and free its worker for one that is wanted.
- Callers that never stop see no change: same calls, same answers, same cost.
- Each stoppable call's body moves into a private function that takes a halt (an optional stop), which both the session's call and the view's call it with. Every new map call has to be added to `Stopping` too, or it can't be stopped; the [render spec](../specs/render.md#stopping) lists the calls that stop.
- Banding heights and the underground probes adds a call per band, at most one per row of pixels; the bench's map lines are checked for a slowdown.
- A raised stop is checked only between units of work, so a call can still run up to one unit (one chunk, one band) after it. The first call that needs a world's template pools can run for seconds after it.
- `Error::Stopped` is a new kind of error. A caller that caches errors (the viewer does, so a failing tile isn't retried every frame) must not cache it.

## Alternatives considered

- **A `stop` parameter on every map call:** the simplest to explain, but it changes every call for every caller, the CLI and oracle included, to serve one front end.
- **A stop held by `Session`:** a session is shared by every job of a seed (`Arc<Session>`), so one stop would stop them all. Cloning a session per job would lose its shared caches (the terrain, the chunk generator and the biome caches).
- **Stop closures threaded into the lower crates:** checks could sit in the innermost loops, but it changes `spyglass-biome` and `spyglass-terrain` APIs for a granularity nobody needs. Bands at the `spyglass` level already give well under half a second.
- **Leaving running jobs to finish, and only dropping their results:** what the viewer did before. A worker stays busy for up to 20 s on a tile nobody will see.
- **Killing or abandoning worker threads:** Rust can't stop a thread from outside. Abandoning one (starting a new worker and ignoring the old one's result) leaves it burning a core until it's done.
