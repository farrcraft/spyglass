# m15. Nearest locate

- Status: Completed
- Milestone: [15. Nearest locate](../../roadmap/completed/15-nearest-locate.md)
- Started: 2026-10-29
- Completed: 2026-10-29

## Goal

A second structure locate that answers with the nearest start, beside the game's `/locate` answer. It covers the library, the `spyglass` command and the viewer's Locate tab. This plan covers all four of the milestone's exit criteria.

## Approach

**Where it lives.** The search goes in `spyglass-structure`, beside `Starts::locate` ([locate.rs](../../../crates/spyglass-structure/src/locate.rs)), as `Starts::nearest(targets, from, radius) -> StructureSearch`. It returns the same `StructureSearch`, with hits nearest first. The API gets its own method, `Session::nearest_structure(&StructureQuery, from, &LocateOptions) -> Result<StructureLocate>`, rather than a mode field in `LocateOptions`. That way the two answers can't be confused, existing callers are unchanged, and `LocateOptions` keeps its meaning: `radius` in rings, and `confirm`. Biome locate is unchanged.

**What "nearest" means.**

- **Distance:** horizontal, to the start's `/locate` position (chunk minimum plus the set's `locate_offset`), as `dx² + dz²` in `i64`. `/locate`'s own comparison is a 3D `distSqr` with the offset's y. The milestone asks for the distance players read, and `blocks_away` is already horizontal.
- **Ties:** by distance, then the set's order of first appearance among the targets (as `/locate` groups them), then chunk z, then chunk x. The order is total, so the answer is deterministic.
- **What's searched:** the same cells `/locate`'s radius covers, per set. For random spread, that's rings `0..=radius` around the cell holding `from`'s chunk. For concentric rings, it's every ring position and its alternatives. So "nearest" means nearest within the area `/locate` would search, and the spec says so.

**The search.**

1. **Ring placements:** take every position and its alternatives, compute their exact distances, sort by the tie order, and evaluate them in that order. Alternatives, and positions that depend on ties or the platform, are candidates only, as in `/locate`'s search. Evaluation stops at the first certain hit.
2. **Random spread, ring by ring:** for each set, compute every cell's potential chunk in ring `r` (`RandomSpread::cell_chunk`). This is cheap: one RNG draw per cell, with no terrain. Then compute the exact distances and drop chunks that fail `is_structure_chunk`. Merge all sets' chunks, sort them by the tie order, and evaluate `targets_at` only while the distance is no greater than the best certain start so far. `targets_at` is the costly step in confirmed mode, so this pruning matters.
3. **Stopping:** after ring `r`, work out the least possible distance to any start in ring `r + 1`, across all sets. For each cell, that's the distance from `from` to the cell's chunk box. The box spans `[cell·spacing, cell·spacing + spacing − separation − 1]` in chunks, times 16, plus the offset. Stop once that bound is greater than the best certain distance, or at `radius`.
4. **The answer:** hits nearest first, holding every candidate nearer than the best certain start (or all of them if there is none), then that start. `certain_answer` points at it. `complete` is false only if more than `MAX_UNCERTAIN` such candidates were found, or a ring position's alternatives are unknown, exactly as for `/locate`. Then the nearest that generates is one of the hits.

Nearest never asks for terrain beyond what `/locate` would at the same chunks. Candidates are decided by the same `targets_at`, so confirmed and unconfirmed behave as they do for `/locate`.

**Front ends.**

- **CLI:** `spyglass locate structure --nearest`. The text output's header says `nearest` instead of naming `/locate`. In JSON, a new `mode` field is `locate` or `nearest`. Adding a field is allowed within format 1 ([cli-json.md](../../specs/cli-json.md)). With `--nearest`, `hits` are nearest first, and `complete` means the nearest is among them.
- **Viewer:** the Locate tab runs both searches for a structure, on the same worker job. If the answers are the same start, it shows one answer, labelled as both. If they differ, it shows "Nearest" first, then "`/locate` gives", each with its distance, and a short note that `/locate` takes the first hit in its ring. Each answer gets a pin, labelled when hovered. Biome locates show one answer, as now.

## Tasks

- [x] `Starts::nearest` in `spyglass-structure`: ring placements, random spread by ring with pruning, the stopping bound, the candidate cap, and the tie order. Unit tests on the ring bound: no start in ring `r + 1` is nearer than it, checked over random seeds, spacings, separations, offsets and origins, including origins near the world border, where chunk arithmetic wraps.
- [x] `Session::nearest_structure` in `spyglass`, with the same `blocks_away` as `locate_structure`. Doc comments cite `docs/specs/locate.md`.
- [x] Fixture tests in `crates/spyglass/tests/synthetic.rs`:
  - For many origins on the structured fixture, the nearest certain start equals the nearest certain start of a brute-force `structures_in` over the disc, with the same tie order, confirmed and unconfirmed.
  - The two modes differ where the ring rule says they should. Build a case where `/locate`'s first-in-scan-order hit is farther than another hit in the same ring.
  - Mixed tags (`#minecraft:everything`): sets with different spacings. A ring set (keeps).
  - The candidate cap with huts, which are never certain.
  - A query from another world is `Error::OtherWorld`. A radius over `MAX_LOCATE_RADIUS` is `Error::TooLarge`.
- [x] An ignored real-bundle test in `crates/spyglass/tests/locate.rs`. For every golden locate case of every supported version:
  - The nearest answer is certain, unless it's a stronghold behind a tie- or platform-dependent position. Those are listed.
  - Confirmed `structures_in` over the bounding box of the disc it claims holds no nearer certain start.
  - Its distance is no greater than `/locate`'s answer's.
  - The cases where the two differ are printed, so they can be listed in this plan's notes.
- [x] CLI: `--nearest` in [args.rs](../../../crates/spyglass-cli/src/args.rs) and the locate output. JSON `mode`. CLI tests for both modes, text and JSON.
- [x] Viewer: [locate.rs](../../../crates/spyglass-viewer/src/locate.rs) runs both and returns both answers. The Locate tab shows them as above. Pins for each. Extend the test `answers_are_the_same_as_the_library` to cover the nearest answer.
- [x] `examples/bench.rs`: `nearest_structure #village`, `minecraft:mansion` and `minecraft:ancient_city`, confirmed, beside the existing `locate_structure` lines. Record before and after in the notes.
- [x] Docs: locate.md, cli-json.md and the getting-started CLI and viewer sections. Changelog entries: Added (library, CLI, viewer).
- [x] `cargo xtask check` and `cargo xtask oracle-test` for every supported version. Fill in the notes. Complete the plan and the milestone.

## Verification

- **Correctness against brute force:** fixture tests compare the nearest answer with a brute-force scan of `structures_in`, over many origins, both modes of confirmation and every placement kind the fixtures have.
- **Real bundles:** the ignored test checks every golden case's nearest answer against `structures_in` over its disc. It runs in confirmed mode, so terrain decides certainty as the game would.
- **`/locate` unchanged:** the existing `/locate` comparison (`cargo xtask oracle-test`) still passes for every version, since `locate_structure` isn't touched.
- **Cost:** bench lines for nearest against `/locate` on the same targets. The plan's target is that nearest takes no more than twice `/locate`'s time for `#village` and `ancient_city` from the origin, confirmed, on seed 0.

## Durable outputs

- [x] [locate.md](../../specs/locate.md): a "Nearest" section covering the distance, the tie order, the cells searched, the stopping bound, the meaning of `complete` and `certain_answer`, and testing. A note under Search order that `/locate`'s answer isn't always the nearest, with the seed 12345 ancient-city example.
- [x] [cli-json.md](../../specs/cli-json.md): `mode`, and what `hits`, `complete` and `certain_answer` mean in nearest mode.
- [x] No ADR is expected. The API shape (a method, not a mode field) is recorded in the spec. If the viewer's two-answer display becomes a pattern for other answers, that would be ADR material.

## Notes

- 2026-10-29: **Results.**
  - **Agreement:** on the real bundles, in every golden structure case of 26.2, 26.3, 26.4-snapshot-5 and 26.4-snapshot-6, no certain start is nearer than the nearest answer. The game's own answer is never nearer, unless it's one of the candidates listed before the nearest.
  - **How often they differ:** the nearest is nearer than `/locate`'s answer in 92 of 264 cases (26.2) and 96 of 276 (each 26.3 and 26.4 bundle). The ignored test prints every one (`cargo test --release -p spyglass --test locate nearest -- --ignored --nocapture`). They're spread across every structure type and all three dimensions, and each is explained by the ring rule: `/locate` keeps a ring's first hit in scan order.
  - **Candidates first:** 4 cases (26.2, 26.3) and 1 (26.4), all strongholds behind tie- or platform-dependent ring positions. There, `structures_in` counts the chunk as certain but the search lists it as a candidate, as `/locate` does. The tests allow a nearer start that's listed as a candidate.
  - **Cost:** `cargo xtask bench`, confirmed, new sessions, 26.3. `#village`: `/locate` 2.5 ms, nearest 3.3 ms. Ancient city: 0.43 and 0.45 ms. Mansion: 2.9 and 3.4 ms. All well under the target of twice `/locate`'s time.
- 2026-10-29: **Ties.** The fixture test first assumed "different answer means strictly nearer". It failed where two towers were at the same distance: `/locate` kept the first in scan order, nearest the lower z. The tie order is as specified; the test now counts only strictly nearer answers as differences.
- 2026-10-29: **Wrapping.** Distances are compared as `i128`. Squared differences of `i32` positions overflow `i64` near the edges of the coordinate range, and fixture origins at the world border exercise that.

- 2026-10-29: the side-by-side comparison with Chunkbase found that its "nearest" differs from `/locate` in 16 of 69 golden cases. In every case both answers generate. The ancient-city case (seed 12345, 26.3: `/locate` gives −288, −288 at 407 blocks, the nearest is 176, −176 at 249) comes from the ring rule: both cells are in ring 1, and `dx = −1` is scanned first.
