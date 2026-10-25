# 0013. Structure templates in bundles, read with our own NBT reader

- Status: Accepted
- Date: 2026-10-25

## Context

Most Overworld structures check their biome at a position derived from a structure template: villages, pillager outposts, abandoned camps, trail ruins, ancient cities and trial chambers at their start piece's bounding-box centre, and ruined portals at their template's. The bounding box comes from the template's size and, for ancient cities, from a named jigsaw block in it. Without templates, spyglass can't say which biome the game checks, so those structures can only be reported as unfiltered candidates. The details are in [structure-placement.md](../specs/structure-placement.md).

Templates are `data/minecraft/structure/**/*.nbt` in the game jar: 1,710 files, 4.2 MB in 26.3. Each is gzip-compressed NBT, Minecraft's binary tag format. [ADR 0006](0006-bundle-format-raw-json-tree.md) already expected them to join the bundle as a new directory, with no format change. What it didn't settle is how to read them.

Forces:

- NBT is a small, stable, documented format: 13 tag types, big-endian, with "modified UTF-8" strings. World saves are NBT too (`level.dat`, region files), and milestone 04 reads those, so whatever reads templates will be reused.
- Gzip needs an inflater. `flate2` is already in the dependency tree through `zip`, with the `zlib-rs` backend the workspace configures.
- Rust NBT crates exist (`fastnbt`, `simdnbt`, `quartz_nbt`, `valence_nbt`). They differ in maintenance, serde coupling and how strictly they handle modified UTF-8 and edge cases.

## Decision

We will:

1. **Bundle templates.** The extractor copies `data/minecraft/structure/` byte for byte, like the other directories. Loaders that need templates fail with a clear error on an older bundle that lacks them.
2. **Read NBT with our own reader** in `spyglass-data` (`spyglass_data::nbt`): a small, hand-written parser into a tag tree, with modified UTF-8 decoding and errors that name the path. It knows the format, not the meaning of any file, like the bundle's JSON access ([ADR 0010](0010-typed-loading-of-worldgen-json.md)).
3. **Inflate gzip with `flate2`** as a direct dependency of `spyglass-data`, on the workspace's existing `zlib-rs` backend.

Each layer parses the tags it needs, in the crate that uses them: `spyglass-structure` reads template sizes and jigsaw blocks.

## Consequences

- Jigsaw start positions, and so the biome check of every jigsaw structure whose start isn't projected to a heightmap, become exact without terrain. Projected ones need terrain only for y.
- Bundles grow by 1,710 files and 4.2 MB. Bundles extracted before this change must be re-extracted to use template-dependent features. `format` doesn't change, so old bundles still open.
- The NBT reader is a few hundred lines that we own and test (round-trips against hand-built bytes, real templates through the ignored bundle test). There's no third-party crate's API or serde model to fit.
- `flate2` becomes a direct dependency. It was already compiled, so builds don't grow. Its license (MIT or Apache-2.0) is already allowed.
- The same reader serves `level.dat` and region files later. Region files add their own chunk framing on top, which is milestone 04's work.

## Alternatives considered

- **An NBT crate (`fastnbt` and others):** less code, but adds a dependency for a format that takes an afternoon to parse. Their APIs lean on serde derive, which ADR 0010 rejected for codec-shaped data. We'd also be tied to their handling of modified UTF-8 and malformed input.
- **Convert templates to JSON at extraction:** keeps bundles JSON-only, but breaks ADR 0006's byte-for-byte rule. The converted files would also no longer be the game's files, so a diff against the jar stops being trivial.
- **Extract only template sizes and jigsaw blocks into a derived file:** small and fast, but it's a spyglass-specific format to version and keep in step, and later features (jigsaw bounds, block-level rendering) need more of each template.
- **Leave templates for milestone 07:** keeps this milestone smaller, but leaves the most common structures (villages, outposts, abandoned camps) as unfiltered candidates until then.
