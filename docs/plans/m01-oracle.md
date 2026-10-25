# m01. Oracle harness and golden biome grids

- Status: Draft
- Milestone: [01. Extractor and oracle harness](../roadmap/01-extractor-and-oracle.md)
- Started: —
- Completed: —

## Goal

A harness that asks the real 26.3 game for biomes and writes golden files for a fixed seed set. Covers the milestone's second exit criterion and the oracle parts of the third and fourth.

This plan is in two phases. Phase 1 is a spike that ends in an ADR choosing the harness approach. Phase 2 is sketched here and **gets rewritten once that ADR is accepted**.

## Phase 1: choose the approach

Options and what's already known are in [research/oracle-approaches.md](../research/oracle-approaches.md). The main contenders:

- **A. Fabric mod** running in a dedicated server.
- **B. Standalone Java harness** compiled against the unobfuscated server jar, with no mod loader: bootstrap the game's registries and call the biome source directly.

### Spike tasks

- [ ] Install a JDK 25; the harness has to be compiled, so a runtime isn't enough. Launcher-bundled runtimes don't include `javac`. Android Studio's bundled JetBrains Runtime does, but it changes with Android Studio updates, so use a standalone JDK (`winget install EclipseAdoptium.Temurin.25.JDK`). Add a JDK check to the setup scripts once the ADR confirms Java is needed.
- [ ] **B first**, since it's cheaper if it works. Write the smallest Java program that, with `server-26.3.jar` on the classpath, bootstraps the game, loads the vanilla worldgen registries, builds the overworld `RandomState` for a seed, and prints the biome at one position. Record which internal classes it touches and how long bootstrap takes.
- [ ] **A.** Generate a Fabric 26.3 mod from the template (Loom, loader 0.20.1) that does the same lookup from a server-started hook. Record setup effort, build time and server start time.
- [ ] Spot-check both against the game at a handful of positions: `/locate biome`, F3, or MiniHUD's biome readout in a client on the same seed. The dev machine already has a Fabric 26.3 client profile with Fabric API `0.162.0+26.3`, Carpet and MiniHUD.
- [ ] Measure throughput for both: biome samples per second at quart resolution.
- [ ] Note what each needs in CI: Java 25, the jar download, Gradle, and whether a server process has to run.
- [ ] Write the ADR. Update the research doc with what the spike found.

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

- 2026-10-24: The Fabric ecosystem is current for 26.3: the dev machine's playing profile runs Fabric API `0.162.0+26.3`, Carpet `26.3+v260915` and Sodium. That's evidence for option A's version-lag question, at least for releases.
- 2026-10-24: Plan drafted. Approach left open pending the spike.
