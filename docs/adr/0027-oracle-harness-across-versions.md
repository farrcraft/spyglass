# 0027. The oracle harness across versions

- Status: Proposed
- Date: 2026-10-26
- See also: [0004](0004-oracle-standalone-harness.md), which this extends; [0026](0026-how-game-versions-coexist.md)

## Context

The oracle harness ([ADR 0004](0004-oracle-standalone-harness.md)) is about 2,400 lines of Java in `oracle/src/` that call the game's internals directly. They cover registry loading, the biome resolver, density sampling and volumes, the spawn finder, heights and structure starts. Each version changes some of those internals. Compiled against each jar it needs to support:

- **26.4-snapshot-5:** 9 errors. `BiomeResolver` became `NoiseBiomeResolver`, `getBaseHeight` is gone, `NoiseColumn` moved and marks solid blocks with `null`, and the signatures of `GenerationContext.biomeResolver` and `Structure.generate` changed.
- **26.2:** 95 errors. 26.3's density API (`DensityBuffer`, `DensityVolume`, `SamplerContext`, `DensitySamplerSet`), `NoiseSpawnFinder` and `synth.Noise` don't exist. The biome resolver takes a sampler, and the spawn search is `Climate$SpawnFinder`.

Most of the harness is version-independent: argument parsing, seeds, case lists, golden-file writing and tie search. Every supported version must produce the same golden formats, so one Rust comparison reads them all. A snapshot a week means the harness must follow a new jar in minutes when only a few calls changed.

## Decision

We will split the harness into a core and adapters:

- **The core** (`oracle/src/`) holds everything that doesn't call version-specific API. It talks to the game through one Java interface, `GameApi`, which provides:
  - loading a world's registries and dimensions;
  - a biome resolver for a chunk;
  - sampling a density function at a point, over a column and over a chunk volume;
  - the spawn search;
  - heights and base columns;
  - starting a structure in a chunk.
- **Adapters** (`oracle/adapters/<first version>/`) implement `GameApi` for a range of versions. Each declares the first data version it supports. `cargo xtask oracle` reads the jar's data version, picks the adapter with the highest first version not above it, and compiles the core with that adapter alone.
  - A version gets a new adapter only when the previous one doesn't compile against it.
  - Small differences inside a range are runtime checks in the adapter, not copies.
- **Ranges now:** `26.2` for the 1.21-family API, `26.3`, and `26.4` from the first 26.4 snapshot.
- **The probe's density cases go through `GameApi` too.** The 26.2 adapter samples volumes through its `NoiseChunk`, the way that version's generator does. The probe file format doesn't change.
- **The split is checked by output.** After the split, the 26.3 adapter must regenerate `oracle-out/26.3` byte for byte.

## Consequences

- **A snapshot that renames a few calls costs one new or edited adapter**, usually small. The size of each adapter, logged per version in the guide's cost log, measures how well `GameApi` is drawn. A large adapter for a small game change means the interface is in the wrong place.
- **Each adapter only compiles against its own range.** `xtask` builds what it picks, so a stale adapter can go unnoticed. CI's matrix compiles every supported version, which catches it.
- **The core can't call game API outside `GameApi`.** Anything new the oracle needs goes through the interface and every live adapter. That makes adding a check more work, but it's the point of the split.
- **26.2's adapter is the largest.** Its density path is the 1.21 one ([ADR 0026](0026-how-game-versions-coexist.md)).
- **Dropping a version deletes its adapter** once no supported version falls in its range.

## Alternatives considered

- **A copy of the harness per version:** no interface to design, but every fix and every new check has to land in each copy, and copies drift. The core is most of the code.
- **Reflection everywhere:** one source tree that compiles against anything and resolves calls at runtime. Mismatches surface as runtime errors deep in a run instead of compile errors, and the code is unreadable. Reflection stays a tool inside an adapter, as 26.2's private `Climate$SpawnFinder` needs.
- **A Fabric mod per version** ([ADR 0004](0004-oracle-standalone-harness.md)'s fallback): the mod would see the same internals and need the same per-version changes, plus loader support for each snapshot.
