# m01. Oracle harness and golden biome grids

- Status: In progress (phase 1 done; ADR 0004 accepted)
- Milestone: [01. Extractor and oracle harness](../roadmap/01-extractor-and-oracle.md)
- Started: 2026-10-24
- Completed: —

## Goal

A harness that asks the real 26.3 game for biomes and writes golden files for a fixed seed set. Covers the milestone's second exit criterion and the oracle parts of the third and fourth.

This plan is in two phases. Phase 1 is a spike that ends in an ADR choosing the harness approach. Phase 2 is sketched here and **gets rewritten once that ADR is accepted**.

## Phase 1: choose the approach

Options and what's already known are in [research/oracle-approaches.md](../research/oracle-approaches.md). The main contenders:

- **A. Fabric mod** running in a dedicated server.
- **B. Standalone Java harness** compiled against the unobfuscated server jar, with no mod loader: bootstrap the game's registries and call the biome source directly.

### Spike tasks

- [x] JDK 25 for the spike: used Android Studio's bundled JetBrains Runtime 25.0.3, which has `javac`, rather than installing one. A standalone JDK is still the recommendation for ongoing work; see ADR 0004.
- [x] **B**: bootstraps, loads registries the way `WorldLoader` does, builds the Default overworld, and resolves biomes through the chunk path. About 80 lines. Bootstrap 8.5 s, registries 1.2 s.
- [~] **A**: not prototyped. B matched real worlds exactly, which removed A's confidence advantage; it's kept as the fallback in ADR 0004.
- [x] Checked against the game: instead of a handful of manual spot checks, the harness was compared with **27.6 million** biome cells stored in two real 26.3 worlds (`Seed Check`, `Seed Check (1)`). Exact everywhere except exact ties; see the research doc.
- [x] Throughput: about 1.5 million cells per second (chunk path) or 130,000 (point path), one thread.
- [x] CI needs: a JDK 25, and the jar downloaded from Mojang per run. No Gradle, no server process, no EULA.
- [x] ADR 0004 written (Proposed). Research doc updated.

### Questions the spike must answer

- Can B build a correct `RandomState` without a world on disk and a running server? If it needs most of server startup, B loses its main advantage.
- How much game-internal API does each approach touch? That's what breaks on version bumps.
- Does A's mod-loader lag behind new releases and snapshots enough to matter? Fabric supported 26.3 at release; check how quickly it supports snapshots.
- Which one is simpler to run in CI?

## Phase 2: harness and golden files (sketch)

- Harness lives in `oracle/` at the repo root. The pre-commit hook allows Java there only. Build tooling follows the ADR.
- Command shape: `dump-biomes --version 26.3 --seed <s> --region <x0,z0,x1,z1> --y <levels> --out oracle-out/`.
- Golden files go to `oracle-out/` (gitignored), never committed, since they're derived from the game.
- Golden file format and seed set are specified in `docs/specs/oracle.md` before the dumper is written.

### Draft seed set and sampling

To be finalized in the spec:

- Seeds `0`, `1`, `-1`, `i64::MIN`, `i64::MAX`, a seed from a string (`"spyglass"`, via `String.hashCode`), and a handful of random 64-bit seeds.
- Quart-resolution grid over a 2048 × 2048 block area at the origin, at several Y levels (biomes are 3D since 1.18).
- Small patches far from the origin (around ±1,000,000 and near the world border) to catch overflow and precision bugs.

### Tasks (tentative)

- [ ] Golden file format and seed set in `docs/specs/oracle.md`.
- [ ] Harness per the ADR, with the `dump-biomes` command.
- [ ] Script to regenerate all golden files for a version (`scripts/oracle.ps1` / `.sh`).
- [ ] Rust reader for golden files in a test-support location, ready for milestone 02.
- [ ] Decide how CI gets the jar without redistributing it: download from Mojang during the CI run, and cache.

## Verification

- Spot checks against the running game agree at every sampled position.
- Regenerating golden files twice gives byte-identical output.
- The Rust reader round-trips a golden file.

## Durable outputs

- [ ] ADR: oracle approach
- [ ] `docs/specs/oracle.md`: format, seed set, open questions resolved
- [ ] `docs/research/oracle-approaches.md`: spike findings
- [ ] `docs/getting-started.md`: how to build and run the oracle

## Notes

- 2026-10-24: Phase 1 done. Key findings for phase 2:
  - Golden data must mark **ties**: cells where several biomes share the minimum fitness. At those cells the game's answer depends on thread scheduling, so comparisons accept any tied biome. A cheap tie check is needed, since brute force over 7,594 points per cell is too slow for full grids.
  - Use `createResolverForChunk` per chunk, as `ChunkGenerator.doCreateBiomes` does. It's about 10× faster than the point path, and it's the path the game uses.
  - Players' own 26.3 saves are a free second check. The spike's Python NBT reader and region dumper are worth turning into a Rust test helper.
  - Spike code is in the session scratchpad (`spike-b/src/SpikeB.java`, `tools/nbt.py`, `tools/dump_biomes.py`). It isn't committed; phase 2 rewrites it properly in `oracle/`.
- 2026-10-24: The Fabric ecosystem is current for 26.3: the dev machine's playing profile runs Fabric API `0.162.0+26.3`, Carpet `26.3+v260915` and Sodium. That's evidence for option A's version-lag question, at least for releases.
- 2026-10-24: Plan drafted. Approach left open pending the spike.
