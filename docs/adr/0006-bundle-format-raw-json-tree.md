# 0006. Version bundle format: raw JSON tree with a manifest

- Status: Accepted
- Date: 2026-10-25

## Context

Every game version spyglass supports needs its worldgen parameters, because parameters live in data and algorithms live in code (CLAUDE.md). The extractor, `spyglass-extract`, builds these parameters into a _version bundle_ from the user's own server jar. Nothing from Mojang may be committed, so each user builds bundles locally into the gitignored `/bundles`.

The inputs come from two places ([specs/data-sources.md](../specs/data-sources.md)):

- **JSON in the jar.** For 26.3: `data/minecraft/worldgen/` (1,144 files: noise settings, density functions, noises, biomes, structures, structure sets, template pools and more), biome and structure tags under `data/minecraft/tags/worldgen/` (123 files), and `data/minecraft/dimension_type/` (4 files).
- **Data-generator reports.** The overworld and nether biome parameter lists are built in code, so they come from `--reports` as `reports/biome_parameters/minecraft/{overworld,nether}.json` (3.4 MB together). The report writes each quantized long as a float, and recovering the long needs an exact inverse (round to nearest), not the game codec's truncation.

What makes the choice matter:

- **Versions keep coming.** Supporting a new version or snapshot is meant to be "extract, diff against the last bundle, fix what changed". The bundle format decides how easy that diff is.
- **Loaders don't exist yet.** The noise, density-function, biome and structure crates that will read these parameters are written in later milestones. A format tied to their Rust types would change every time they do.
- **Integrity.** A bundle is derived from a specific jar. Tools and tests need to know which one, and to detect a bundle that was edited or partly written.
- **Load cost.** Loading 26.3's files is a few megabytes of JSON, read once per process. The viewer and search read it at startup, not per seed.

## Decision

We will store a bundle as a **directory tree of the game's JSON files, byte for byte, with game paths preserved, plus a `bundle.json` manifest**:

```text
bundles/26.3/
  bundle.json
  data/minecraft/worldgen/...
  data/minecraft/tags/worldgen/...
  data/minecraft/dimension_type/...
  reports/biome_parameters/minecraft/overworld.json
  reports/biome_parameters/minecraft/nether.json
```

`bundle.json` records the bundle format version, the game version id, the data version (`world_version` from the inner jar's `version.json`), the server jar's SHA-1, the Java version that ran the data generator, the extractor's version, and the SHA-1 of every other file in the bundle. It holds nothing that varies between runs (no timestamps), so extracting the same jar twice gives identical bundles. `spyglass-extract verify` rechecks the hashes.

Files are not reformatted, normalized or converted. In particular, the biome parameter reports keep their floats, and the loader converts them back to quantized longs with the exact inverse given in the data-sources spec.

The layout and fields are specified in [specs/bundle-format.md](../specs/bundle-format.md).

## Consequences

- Diffing two versions is `diff -r bundles/26.2 bundles/26.3`, or any directory diff tool, with no spyglass tooling involved. Changes show up per file, in the game's own names.
- Anything that knows the game's datapack layout can read a bundle. Paths match vanilla datapacks, so a later feature that loads user datapacks can reuse the same loader.
- The format doesn't depend on any spyglass crate's types, so loaders can evolve without bumping the bundle format.
- Loading parses JSON at startup rather than mapping a ready-made binary. That costs milliseconds to tens of milliseconds for 26.3's few megabytes. If it ever matters (for example, many versions loaded at once), a compiled cache can be built from the tree later, derived and disposable, without changing this format.
- A bundle is about 1,300 small files rather than one. Copying it around is clumsier, and some file systems are slow with many small files. Bundles are local build products, so this mostly doesn't matter.
- Every loader has to handle the game's JSON as it is, including codec shorthands (a density function given as a constant number or a registry reference, a biome parameter given as a single number or a pair). That work is needed anyway to read user datapacks.
- File hashes use SHA-1, which `spyglass-data` already depends on for jar verification. They check integrity, not authenticity, so SHA-1's weakness against deliberate collisions doesn't matter here.
- Which directories a bundle includes will grow. Structure templates (`data/minecraft/structure/`, NBT) will be needed for jigsaw structure bounds. Adding a directory is a spec change and an extractor-version change, not a new format.

## Alternatives considered

- **Compiled single file** (bincode, MessagePack or a custom binary of parsed parameters): one file, fastest to load, already validated. But it is opaque to diff, so the "extract, diff, fix" workflow needs a custom dump tool. Its schema is the loaders' Rust types, which don't exist yet and would version the format every time they change. Kept as a possible derived cache, above.
- **One combined JSON file** (all inputs merged into a single document keyed by path): one file and still text, but diffs become one huge file, the game's paths turn into keys, and nothing else can read it as a datapack.
- **No bundle: read the cached jar at runtime.** The JSON is already in the jar, but the biome parameter lists need the data generator and a Java runtime, which shouldn't be needed every time the viewer starts. Running the generator once at extraction time and keeping the result is what a bundle is.
- **Store quantized longs instead of the report's floats:** removes the float inverse from the loader. But it means the extractor rewrites game output instead of copying it, which makes bundles differ from what the game produced and puts parsing logic in the extractor. The inverse is a few lines, exhaustively checked, and specified once.
