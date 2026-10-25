# m03. Public API, locate queries and the CLI

- Status: Draft
- Milestone: [03. Structure placement and locate queries](../roadmap/03-structure-placement.md)
- Started: —
- Completed: —

## Goal

The `spyglass` facade crate, with one session object and locate queries, and the `spyglass` command over it, with human-readable and `--json` output. Covers the milestone's third, fifth and sixth exit criteria. Completing this plan completes the milestone.

The last of three plans for milestone 03. Needs [m03-placement](completed/m03-placement.md); the spawn command needs [m03-spawn](m03-spawn.md).

## Facts established up front

Checked against the 26.3 jar (`javap`) on 2026-10-25.

- **`/locate structure`** calls `ChunkGenerator.findNearestMapStructure`. It groups the target structures by placement and asks each placement for its nearest generated structure: `getNearestGeneratedStructure` over the ring positions for concentric rings, or a ring-by-ring search over placement cells for random spread. It keeps the closest by `BlockPos.distSqr`. "Generated" means a real start, through `StructureManager`, so the game's answer includes terrain-dependent validity.
- **Search order matters.** The random-spread search stops at the first ring of cells with a hit, so `/locate`'s answer isn't always the Euclidean nearest. Agreeing with `/locate` means porting its search order and distance measure, not computing a true nearest.

## Approach

1. **ADR** for the public API: a `Session` (bundle + seed + dimension; later, a world save) that owns the loaded sources and exposes queries; front ends depend only on `spyglass`. It also covers the library-first split between the API, the CLI and the GUI ([architecture.md](../architecture.md#front-ends)), result types that carry `Candidate` vs confirmed and tie information, and errors. Written before the crate.
2. **`crates/spyglass`**: `Session::new(bundle, seed, dimension)`, `biome_at`, `structures_in(set or structure, area)`, `nearest_structure(kind, from)`, `structures_within(kind, from, radius)`, `nearest_biome(biome, from)`, `is_slime_chunk`, `spawn_origin`. Locate queries follow the game's search order. Results are labelled candidates where the biome check needs terrain.
3. **`crates/spyglass-cli`** (binary `spyglass`, `clap`): `spyglass locate structure|biome|slime|spawn`, with `--version`, `--seed` (text seeds hashed as the game does), `--from x,z`, `--radius` and `--json`. The bundle comes from `bundles/<version>/`, or `--bundle`. The bin target sets `doc = false` so `cargo doc` doesn't collide with the `spyglass` library.
4. **`--json` spec**: one documented, versioned schema per command, with a `format` field, because scripts depend on it.
5. **Spot-checks against `/locate`**: an oracle `locate` subcommand that calls `findNearestMapStructure` and `findClosestBiome3d` for a list of (seed, kind, position) cases, and an ignored test comparing them with the API.
6. **Viewer**: switch `spyglass-viewer` from the lower crates to `spyglass`.

## Tasks

- [ ] Disassemble `findNearestMapStructure`, both `getNearestGeneratedStructure` overloads and `BiomeSource.findClosestBiome3d` (radius, steps, order) used by `LocateCommand`.
- [ ] ADR: public API shape and the library-first split (`adr` skill).
- [ ] `crates/spyglass`: session and queries, with unit tests on small synthetic bundles where possible.
- [ ] `crates/spyglass-cli`: commands, human output and `--json`. Integration tests running the binary on a bundle (ignored when no bundle is present).
- [ ] `docs/specs/cli-json.md`: the JSON output format.
- [ ] Oracle: `locate` subcommand and a fixed case list; ignored comparison test. Answers whose game result depends on terrain validity are compared as "the game's answer is among our candidates, in order".
- [ ] `spyglass-viewer` depends on `spyglass` only; update [architecture.md](../architecture.md#crates) (remove _Planned_ from `spyglass` and `spyglass-cli`, drop "Until `spyglass` exists").
- [ ] `docs/getting-started.md` and the README: using the CLI.
- [ ] Milestone wrap-up: tick the exit criteria, move the m03 plans and the milestone to `completed/`, update the roadmap and plan indexes, fix links.

## Verification

- CLI integration tests on each command, in both output modes.
- The `/locate` comparison passes at every case in the list.
- `cargo xtask check` passes with the new crates (lints, unused deps, `cargo doc`, MSRV, `cargo-deny`).

## Durable outputs

- [ ] ADR: public API shape
- [ ] `docs/specs/cli-json.md`, listed in `docs/specs/README.md`
- [ ] `docs/specs/oracle.md`: locate cases
- [ ] `docs/architecture.md`: crates table and front ends
- [ ] `docs/getting-started.md`, README: CLI usage

## Open questions

- **How far locate searches.** The game caps the random-spread search (100 cells for `/locate`). Match that by default and allow a larger `--radius`?
- **Candidates in locate answers.** Until milestone 07, "nearest village" may be a start the game rejects on terrain. Return the nearest candidate and say so, or the nearest few?

## Notes

- 2026-10-25: Plan drafted.
