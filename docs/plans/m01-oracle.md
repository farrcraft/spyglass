# m01. Oracle harness and golden biome grids

- Status: In progress (phase 1 done; ADR 0004 accepted)
- Milestone: [01. Extractor and oracle harness](../roadmap/01-extractor-and-oracle.md)
- Started: 2026-10-24
- Completed: —

## Goal

A harness that asks the real 26.3 game for biomes and writes golden files for a fixed seed set. Covers the milestone's second exit criterion and the oracle parts of the third and fourth.

This plan is in two phases. Phase 1 was a spike that ended in [ADR 0004](../adr/0004-oracle-standalone-harness.md): a standalone Java harness on the server jar. Phase 2 builds that harness properly.

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
- [x] ADR 0004 written and accepted. Research doc updated.

The spike's questions are answered in the research doc's "Spike results" section.

## Phase 2: harness and golden files

### Shape

- **Java side, `oracle/`.** Plain Java sources in package `spyglass.oracle`, no build tool. One entry point with subcommands. Each follows a named server code path and cites it in comments:
  - `biomes`: dump a region for a seed via `createResolverForChunk` per chunk, marking ties;
  - `check-save`: compare the harness with the biomes stored in a real save. It reads region files with the game's own `RegionFile`/`NbtIo`, so no Rust NBT dependency is needed yet.
- **Rust side, `cargo xtask oracle`:**
  - `build`: fetch and verify the jar (sharing the extractor's jar cache), unpack the bundler into `target/oracle/<version>/classpath/`, find a JDK of the right major with `spyglass_data::java`, and compile with `javac --release <major>`;
  - `biomes` / `check-save`: build if stale, then run the harness;
  - `golden`: regenerate the whole golden set for a version into `oracle-out/<version>/`.
- **Golden files** go to `oracle-out/` (gitignored), never committed. They're regenerated locally and in CI.

### Golden format (proposal; finalized in `docs/specs/oracle.md`)

One file per (seed, region, dimension):

- a small JSON header: version, data version, seed, dimension, quart bounds, Y levels, harness version;
- a biome palette;
- one `u16` palette index per cell in x-z-y order, little-endian;
- a tie table listing each tied cell's index and its full set of tied palette indices.

Binary keeps 2048 × 2048-block grids small (512 × 512 × levels × 2 bytes). The header makes files self-describing for the Rust reader.

### Seed set and sampling (proposal; finalized in the spec)

- Seeds `0`, `1`, `-1`, `i64::MIN`, `i64::MAX`, a string seed (`"spyglass"` via `String.hashCode`), and four fixed random 64-bit seeds.
- A quart-resolution grid over 2048 × 2048 blocks at the origin, at all Y levels (96 quarts tall in the overworld). That's about 25 million cells per seed at about 1.5 million per second, so about 17 s per seed on one thread, before parallelising across seeds.
- 64 × 64-block patches at ±1,000,000 and just inside the world border (±29,999,984), to catch overflow and precision bugs.

### Tasks

- [ ] Finalize the golden format, seed set and tie rule in `docs/specs/oracle.md`.
- [ ] **Tie detection.** An exact search that returns _all_ parameter points at the minimum fitness: a bounding-box tree like `Climate.RTree`, but pruning with `>` rather than `>=` and collecting equals. Validate it against brute force on a random sample of cells, and check it agrees with the spike's tie counts on the two Seed Check worlds.
- [ ] `oracle/` harness: shared bootstrap (registries via the `WorldLoader` sequence, dimensions via `WorldPresets`), then the `biomes` and `check-save` subcommands.
- [ ] `cargo xtask oracle build`, using the extractor's jar cache. This depends on the m01-extractor "jar cache" task, so do that one first.
- [ ] `cargo xtask oracle biomes`, `check-save` and `golden`.
- [ ] `check-save` passes on both Seed Check worlds: all mismatches are ties, with zero unexplained.
- [ ] Rust golden-file reader in a test-support crate or module, ready for milestone 02.
- [ ] Pre-commit hooks for Java: the harness is the repo's only Java, so add a formatter (e.g. google-java-format) only if it earns its keep. Decide while writing the harness.
- [ ] CI design notes: JDK setup, jar download per run, cached by SHA-1. CI itself isn't set up yet, so this is notes only.

## Verification

- `check-save` against real 26.3 worlds: every mismatch is an exact tie.
- Tie detection agrees with brute force on every sampled cell.
- Regenerating golden files twice gives byte-identical output.
- The Rust reader round-trips a golden file.

## Durable outputs

- [x] ADR 0004: oracle approach
- [ ] `docs/specs/oracle.md`: golden format, seed set, tie rule (tie rule done), open questions resolved
- [x] `docs/research/oracle-approaches.md`: spike findings
- [x] `docs/getting-started.md`: JDK setup (`setup -Oracle` / `--oracle`)
- [ ] `docs/getting-started.md`: building and running the oracle

## Notes

- 2026-10-24: Phase 1 done. Key findings for phase 2:
  - Golden data must mark **ties**: cells where several biomes share the minimum fitness. At those cells the game's answer depends on thread scheduling, so comparisons accept any tied biome. A cheap tie check is needed, since brute force over 7,594 points per cell is too slow for full grids.
  - Use `createResolverForChunk` per chunk, as `ChunkGenerator.doCreateBiomes` does. It's about 10× faster than the point path, and it's the path the game uses.
  - Players' own 26.3 saves are a free second check. The spike's Python NBT reader and region dumper are worth turning into a Rust test helper.
  - Spike code is in the session scratchpad (`spike-b/src/SpikeB.java`, `tools/nbt.py`, `tools/dump_biomes.py`). It isn't committed; phase 2 rewrites it properly in `oracle/`.
- 2026-10-24: The Fabric ecosystem is current for 26.3: the dev machine's playing profile runs Fabric API `0.162.0+26.3`, Carpet `26.3+v260915` and Sodium. That's evidence for option A's version-lag question, at least for releases.
- 2026-10-24: Plan drafted. Approach left open pending the spike.
