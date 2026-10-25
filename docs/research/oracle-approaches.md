# Oracle approaches

How to get ground-truth worldgen output from the real game. Background for the oracle ADR. Facts were checked on 2026-10-24 against 26.3.

## What's changed

From 26.x the server jar ships **unobfuscated**: classes and members keep their real names, and the version JSON no longer lists `server_mappings`. Older tools assumed obfuscated jars, which made it costly to call game code directly without a mod loader's remapping. That cost is gone, and it reopens a standalone-harness option.

26.3 requires Java 25.

## Options

### A. Fabric mod in a dedicated server

A server-side mod hooks server start, then samples the world's biome source and chunk generator directly.

- Gets the exact state the game uses: datapacks loaded, `RandomState` created by the server itself. Highest confidence.
- Mature tooling: Fabric Loom (Gradle), and loader 0.20.1 supported 26.3 at release.
- Costs: Gradle and Loom setup, server start time on every run, and mod-loader support for new versions and snapshots. That support arrived quickly for 26.3; how fast it arrives for snapshots is to be measured.
- NeoForge (26.3.0.x) is an equivalent alternative, but Fabric is lighter for a server-only tool.

### B. Standalone Java harness on the server jar

A plain Java program, compiled with the unobfuscated server jar on the classpath, that bootstraps the game and calls the biome source without starting a server.

- No loader, no Gradle required (plain `javac` could work), and no wait for loader support: works on a snapshot the day it ships.
- Fast iteration, with no server process or world on disk, if bootstrap is light enough.
- Risk: the harness has to rebuild what server startup does (registry loading from the vanilla datapack, `RandomState` creation). If that means reproducing most of server startup, the advantage over A shrinks and correctness confidence drops.
- Touches more internal API directly, so it may need fixes on version bumps. A mod faces some of the same churn.

### C. Real server and region files

Run an unmodified server, pre-generate chunks, then read region files (e.g. with `fastanvil`).

- Zero custom game code, and checks the full pipeline, including features.
- Very slow, and only gives what's stored in chunks (biomes are stored at quart resolution, so this does work for biomes). Better suited to heightmaps and blocks in milestone 06 than to bulk biome sampling.

### D. Commands over RCON

`/locate`, `/place` and similar against a running server.

- No custom code, but slow and coarse: one answer per command. Useful for spot checks, not for golden files.

## Open questions

For a prototype spike of A and B to answer:

- How much of server startup does B have to replicate to get a correct `RandomState`?
- Throughput of A vs B in samples per second.
- How quickly Fabric supports snapshots.
- CI cost of each: Java 25, Gradle, the jar download, a server process.
