# m01. Extractor and version bundle

- Status: In progress
- Milestone: [01. Extractor and oracle harness](../roadmap/01-extractor-and-oracle.md)
- Started: 2026-10-24
- Completed: —

## Goal

`spyglass-extract 26.3` produces a version bundle for Minecraft 26.3 from the user's own download, with nothing from Mojang committed. Covers the milestone's first exit criterion and the bundle-format parts of the third and fourth.

## Facts established up front

Checked against the live manifest and the 26.3 server jar on 2026-10-24:

- 26.3 is the latest release (2026-09-15). The version JSON lists `server` (62 MB) with a SHA-1, and `javaVersion.majorVersion = 25`. It has no `server_mappings`.
- The server jar is a bundler. The real jar is at `META-INF/versions/26.3/server-26.3.jar`.
- The inner jar is **unobfuscated** (`net/minecraft/world/level/levelgen/NoiseRouter.class` etc.), so no mappings step is needed.
- `data/minecraft/worldgen/` has 1,234 JSON files across `noise_settings`, `density_function`, `noise`, `biome`, `structure_set`, `structure`, `template_pool` and more.
- `multi_noise_biome_source_parameter_list/overworld.json` is only `{"preset": "minecraft:overworld"}`. The actual parameters are built in code by `OverworldBiomeBuilder`, so they have to come from the data generator. **The extractor therefore needs Java 25.**

## Approach

All in `spyglass-data`: a library for manifest, download and bundle logic, and the `spyglass-extract` binary as a thin CLI over it.

1. **Manifest client.** Fetch `version_manifest_v2.json`, resolve a version id (or `latest`) to its version JSON, and read the server download URL, SHA-1, size and required Java major version. Use `ureq` and `serde`.
2. **Jar cache.** Download into a per-user cache directory (`directories` crate, or `--cache-dir`). Verify the size and SHA-1 (`sha1`) before use, and re-download on mismatch. Never write jars inside the repo.
3. **Jar reading.** Open the bundler jar with `zip`, locate the inner versioned jar, and copy `data/minecraft/worldgen/**` out.
4. **Data generator.** Find a Java runtime whose major version matches the version JSON. Search in this order: `--java`, `JAVA_HOME`, `PATH`, then the runtimes that game launchers already download. Anyone who plays the version has a matching runtime on disk (a JRE is enough; no JDK needed). Known launcher locations:
   - Official launcher: `runtime/<component>/<platform>/<component>/` under the launcher's data folder. On Windows the Microsoft Store build keeps it in `%LOCALAPPDATA%\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime`. The component name comes from the version JSON (`java-runtime-epsilon` for 26.3).
   - Modrinth App: `%APPDATA%\ModrinthApp\meta\java_versions\*`.
   - Others (Prism, CurseForge) to be added as they're confirmed.

   Read each candidate's `release` file for `JAVA_VERSION` rather than running it. Then run `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --output <tmp>` and keep the reports the bundle needs: `reports/biome_parameters/minecraft/overworld.json` and `nether.json`. Run it in a scratch working directory, because the bundler unpacks its jars into the current directory.

5. **Bundle.** Write `bundles/26.3/` (gitignored) per the bundle spec. Proposed shape: a `bundle.json` holding the version id, the data version (`world_version` from the inner jar's `version.json`, 5023 for 26.3, which world saves record in `level.dat`), jar SHA-1, extractor version and a hash of every included file, then the JSON tree with game paths preserved. Raw JSON keeps bundles diffable between versions, which the "extract, diff, fix" workflow depends on. The format is decided in an ADR before it's built.

## Tasks

- [x] Run the data generator on 26.3 by hand with Java 25. Record which reports exist and where the overworld biome parameters land (`docs/specs/data-sources.md`, Data generator).
- [x] Java discovery, including launcher runtimes (`spyglass_data::java`, also used by `cargo xtask doctor`). Only if nothing matches, print install instructions (e.g. `winget install EclipseAdoptium.Temurin.25.JRE`). The setup scripts don't install Java for the extractor.
- [x] Write `docs/specs/bundle-format.md`, and an ADR choosing between raw JSON tree and a compiled single file. ADR 0006 (raw JSON tree) accepted 2026-10-25.
- [x] Manifest client and version JSON types, with unit tests against small hand-written manifest fixtures (`spyglass_data::manifest`).
- [x] Jar cache with SHA-1 verification (`spyglass_data::cache`, done for the oracle).
- [ ] Bundler jar → inner jar → worldgen JSON copy. Tests use a synthetic zip built in the test, not a Mojang jar. Unpacking the inner jar and libraries is done (`spyglass_data::bundler`, for the oracle); the worldgen JSON copy isn't.
- [ ] Data-generator invocation, with a clear error when no matching Java is found that lists where it looked.
- [ ] Bundle writer and `bundle.json`, plus a `verify` subcommand that rechecks the hashes.
- [ ] `spyglass-extract` CLI: `spyglass-extract [VERSION] [--cache-dir] [--java] [--out]`.
- [ ] End-to-end test against real 26.3, marked `#[ignore]` (needs network and Java). Run with `cargo test -- --ignored`.
- [ ] Fill in "Game data" in `docs/getting-started.md`.

## Verification

- Unit tests for everything except the network and Java steps, all using synthetic fixtures.
- The ignored end-to-end test produces a bundle for 26.3 whose file count and `bundle.json` hashes are stable across two runs.
- A fresh clone plus `scripts/setup` plus `spyglass-extract 26.3` works on Windows and one of macOS or Linux.
- `git status` after extraction shows nothing new: bundles and jars stay ignored.

## Durable outputs

- [x] `docs/specs/bundle-format.md`
- [x] ADR 0006: bundle format
- [x] `docs/specs/data-sources.md`: the obfuscation TODO and the Java question resolved (done while writing this plan)
- [ ] `docs/getting-started.md`: extraction steps, and what to do if no matching Java is found
- [x] `docs/specs/data-sources.md`: Java runtime discovery order and launcher locations

## Notes

- 2026-10-25: Ran the data generator by hand (`--reports`, Zulu 25 JRE from the Modrinth App): about 6 s, 1,666 files. Only `biome_parameters/minecraft/{overworld,nether}.json` are needed. The report prints parameters as unquantized floats; recovering the game's longs needs round-to-nearest, not the codec's truncation (spec has the details and the exhaustive check). Decision for the bundle ADR: store the reports as written, and let the loader do the exact inverse, or store quantized longs.
- 2026-10-24: Java discovery and the manifest client landed early, because `cargo xtask doctor` (ADR 0003) needs them.

- 2026-10-24: On the dev machine, a matching runtime was already present from the Modrinth App (`zulu25.34.17-ca-jre25.0.3`), while `PATH` only had Java 17. The Store launcher's runtime folder had 8/17/21 but no `epsilon`, because 26.3 is played through Modrinth there. That's why launcher runtimes are part of discovery.
- 2026-10-24: Plan drafted. Target version 26.3, confirmed unobfuscated.
