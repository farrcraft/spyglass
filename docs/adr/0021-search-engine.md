# 0021. The seed search engine: staged checks on rayon, deterministic ranking

- Status: Accepted
- Date: 2026-10-25

## Context

Seed search evaluates a query ([ADR 0020](0020-search-query-language.md)) against many seeds and keeps the best. The seed space is 2⁶⁴, and the per-seed costs differ by orders of magnitude: structure placement is tens of nanoseconds and depends only on the seed's lower 48 bits ([specs/structure-placement.md](../specs/structure-placement.md#the-lower-48-bits)); binding a seed's climate is about 0.1 ms; the spawn origin about 1 ms; stronghold rings about 1 s.

The milestone requires that every reported seed meets its required constraints, checked against the plain implementations, and that the same query and seed range always give the same ranked list. A search runs on every core and reports its best results while it runs. It must also run inside the viewer without stalling the map, and stop on request.

The architecture places `spyglass-search` below `spyglass` (depending on rng, biome, structure), with front ends reaching it only through `spyglass` ([ADR 0014](0014-public-api-world-and-session.md)).

## Decision

We will build the search in two layers, run it on `rayon`, and make ranking a total order.

- **`spyglass-search`** holds the query language, a **placement prefilter** over the lower 48 bits, seed sources, and the parallel runner with ranking. It doesn't know `Session`: `spyglass` gives it the query's structure targets resolved to set indices, and a per-seed check to call.
- **`spyglass`** holds per-seed evaluation. `Session::evaluate` is the plain implementation, every constraint in full, from the session's own queries. `Session::check` is the fast path the runner calls: the same per-constraint functions, cheapest first, stopping at the first failed required constraint and skipping preferred ones that can't lift the score into the top list. Reported hits are re-evaluated with `Session::evaluate`.
- **Staging by necessary conditions.** A cheaper stage may reject a seed only if the full evaluation would. The placement prefilter uses upper bounds on structure counts (the biome check only removes starts) and exact slime chunks, and rejects all 65,536 seeds sharing a lower 48 bits at once.
- **Deterministic ranking.** Seeds are ranked by score, then by their index in the seed source, which is a total order. Work is split into batches; each batch's best are merged into the running top list in that order. With a seed-count budget the evaluated set is exact, so results don't depend on thread count or timing. Scores are integers. A time budget stops at a point where every index below a reported `next` is done.
- **`rayon` 1.12** for the parallel runner, in its own thread pool sized by the caller (the viewer leaves a core for the UI and tiles). Progress and cancellation go through a callback the runner calls between batches.
- **`ctrlc` 3.5** in `spyglass-cli`, so Ctrl-C ends a search and prints the best so far instead of losing it.

The search semantics, sources and determinism rules are in [specs/search.md](../specs/search.md).

## Consequences

- The optimizations the milestone asks to verify (prefilter, 48 → 64-bit expansion, cost order, early exit, scoring bound) are each compared with `Session::evaluate` over seed ranges in tests. The per-constraint functions themselves are shared, so they're checked by hand-made cases instead.
- Constraint semantics live in one place, `spyglass`. The prefilter in `spyglass-search` duplicates only the placement-count reasoning, and its soundness is tested.
- Every lower-48 value that survives the prefilter costs 65,536 climate bindings, several seconds on one core. Queries that the prefilter can't narrow (biome-only queries) run at the full-seed rate, about 10,000 seeds a second per core at best. Making per-seed binding cheaper is library work, measured before it's done.
- `rayon` (with `rayon-core` and the `crossbeam` crates) and `ctrlc` (with `nix` on Unix; `windows-sys` is already in the tree) join the dependency tree. Both are MIT or Apache-2.0, already allowed in `deny.toml`; `cargo deny` checks the rest. Both support older Rust than the workspace's 1.95.
- The runner owns its pool, so a library user who already has a rayon pool gets a second one. Acceptable for now; a pool can be passed in later.
- A time-limited search's results depend on how far it got, which is why it reports `next`.

## Alternatives considered

- **The whole evaluator in `spyglass-search`**, over the lower crates' types: matches the original crate sketch, but duplicates every constraint's semantics beside `Session`, and an equality test between two hand-written evaluators checks less than one between a plain and a fast path over shared functions.
- **Plain `std::thread` with a hand-rolled queue:** no dependency, but work stealing, splitting and pool sizing are what `rayon` does well, and the viewer's tile workers are a different shape (a most-wanted-first queue) from a batch search.
- **Ranking by score alone, ties by arrival:** fast, but the list would depend on thread timing.
- **Floating-point scores:** natural for graded distances, but summing in a different order could change the last bit and the ranking; integer thousandths avoid the question.
- **No Ctrl-C handling:** a long search interrupted by the user would print nothing; the alternative of streaming every new best to stdout clutters normal output.
