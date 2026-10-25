# 0004. Oracle: standalone Java harness on the server jar

- Status: Accepted
- Date: 2026-10-24

## Context

spyglass's correctness is checked against the real game (see [specs/oracle.md](../specs/oracle.md)). The oracle has to produce large golden datasets (millions of biome cells per seed, and later structures and heights) for each supported version, run locally and in CI, and keep up with new releases and snapshots. Mojang's code and data can't be committed or redistributed.

From 26.x the server jar ships unobfuscated, which makes calling game code directly practical without a mod loader's remapping. A spike compared the options; the full findings are in [research/oracle-approaches.md](../research/oracle-approaches.md). In short:

- A standalone harness of about 80 lines, compiled against `server-26.3.jar` and its bundled libraries, reproduced the biomes stored in two real 26.3 worlds for **27.6 million cells, exactly**, apart from cells tied in the game's own search.
- Those ties (about 1 cell in 2,000) are history-dependent in the game itself, so no oracle can give a single answer for them.
- It runs at about 1.5 million cells per second per thread after about 10 s of startup. It needs no mod loader, no Gradle, no server process, no world on disk and no EULA step.

## Decision

We will build the oracle as a **standalone Java harness** (option B) in `oracle/`, compiled with plain `javac --release <N>` against the user's own server jar and the libraries in its bundler. N is the version JSON's `javaVersion.majorVersion`: 25 for 26.3.

- **Build and run through `cargo xtask oracle`.** It reuses `spyglass-data` to fetch and verify the jar, unpacks the bundler into a classpath, finds a JDK with the existing Java discovery, compiles the harness and runs it. No Gradle, Maven or other build tool.
- **Reproduce the server's own code paths** (`WorldLoader`'s registry loading, `WorldPresets` for dimensions, `ChunkGenerator.doCreateBiomes` for biomes) rather than shortcuts, and cite the game classes followed in comments.
- **Report ties.** Golden data marks cells where the minimum is tied and lists the tied candidates, so the Rust side can be held to "exact where unique, one of the tied set otherwise".
- **Use real 26.3 world saves as a second, independent check** on the harness, as the spike did: comparing against the biomes stored in region files.

A **JDK of the target major version** is required for oracle work only. The extractor and everything else keep needing just a runtime, which launchers already provide.

## Consequences

- No mod loader to wait for. The harness can target a snapshot the day it ships, limited only by how much of the game's internal API changed.
- The build stays inside existing tooling: Rust (xtask) plus a JDK. Contributors who never touch the oracle need no JDK.
- The harness calls game internals directly (`RegistryLayer`, `RegistryDataLoader`, `SamplerContext`, `BiomeSource.createResolverForChunk`), and these change between versions; 26.3 already differs from 1.21. Version bumps will need harness fixes. A mod would have similar churn with an extra layer on top.
- Oracle contributors need a JDK 25. Setup gets an opt-in step for it, and `cargo xtask doctor` already reports whether one is present.
- CI needs a JDK and downloads the jar from Mojang during the run. Nothing derived from the game is committed.
- Later stages need more game state: structure starts with validity checks, jigsaw assembly and terrain. If building that standalone proves impractical, a Fabric mod (option A) is the fallback for those parts. That would be a new ADR, not a quiet change.
- Tie handling is now part of what "bit-exact" means for biomes, and the specs must say so.

## Alternatives considered

- **A. Fabric mod in a dedicated server:** mature tooling (Loom; loader 0.20.1 supported 26.3 at release), and it gets a live server's state for free. But it needs Gradle and Loom, a server start per run, an accepted EULA, and loader support for each new version. Its main advantage, confidence that the state is the game's own, is now matched by B's exact agreement with real worlds. Not prototyped; kept as the fallback above.
- **C. Unmodified server plus region files:** no custom game code, but slow, and limited to what's stored in chunks. Kept as the independent check above, using worlds that already exist rather than generating new ones.
- **D. Commands over RCON:** one answer per command. Too coarse for golden data.
