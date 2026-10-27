# Version bundle format

Status: stable. The format is decided in [ADR 0006](../adr/0006-bundle-format-raw-json-tree.md).

A version bundle holds every worldgen parameter spyglass needs for one game version. `spyglass-extract` builds it locally from the user's own server jar ([data-sources.md](data-sources.md)). Bundles are never committed: they live in `/bundles`, which is gitignored.

## Layout

A bundle is a directory, by default `bundles/<version id>/`:

| Path                                  | From                                                                         | 26.3 files |
| ------------------------------------- | ---------------------------------------------------------------------------- | ---------- |
| `bundle.json`                         | The extractor (below).                                                       | 1          |
| `data/minecraft/worldgen/**`          | The inner game jar, same path.                                               | 1,144      |
| `data/minecraft/tags/worldgen/**`     | The inner game jar, same path.                                               | 123        |
| `data/minecraft/tags/block/**`        | The inner game jar, same path: block tags, for heightmaps.                   | 297        |
| `data/minecraft/dimension_type/**`    | The inner game jar, same path.                                               | 4          |
| `data/minecraft/structure/**`         | The inner game jar, same path: structure templates, gzip-compressed NBT.     | 1,511      |
| `reports/biome_parameters/minecraft/` | The data generator's `--reports` output: `overworld.json` and `nether.json`. | 2          |

Rules:

- Files are copied **byte for byte**. Nothing is reformatted, normalized or converted.
- Every file under the included directories is copied, whatever its extension.
- Paths use `/` and are relative to the bundle root, both on disk and in `bundle.json`.
- Nothing else is in the bundle directory. A file that isn't listed in `bundle.json` makes the bundle invalid.

The set of included directories is part of the format. Adding one changes this spec and the extractor's version, but not `format`. Structure templates were added this way ([ADR 0013](../adr/0013-structure-templates-and-nbt.md)), then block tags. A bundle extracted before then has none, and loaders that need them say so and ask for it to be extracted again.

## `bundle.json`

UTF-8 JSON, pretty-printed with two-space indent, keys in the order below, and a trailing newline:

```json
{
  "format": 1,
  "version": "26.3",
  "data_version": 5023,
  "kind": "release",
  "jar_sha1": "33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c",
  "java_version": "25.0.3",
  "extractor": "0.1.0",
  "files": {
    "data/minecraft/dimension_type/overworld.json": "…",
    "reports/biome_parameters/minecraft/overworld.json": "ed8590a657f04acb51074c2082d2b97c81a56f70"
  }
}
```

| Field          | Meaning                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `format`       | Bundle format version, an integer. This spec describes `1`. Readers reject formats they don't know.                                                                                                                                                                                                                                                                                                                                                                                                         |
| `version`      | The game version id, as in the version manifest.                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `data_version` | `world_version` from the inner jar's `version.json`. World saves record the same number in `level.dat` as `DataVersion`.                                                                                                                                                                                                                                                                                                                                                                                    |
| `kind`         | `release` or `snapshot`, from the version manifest's `type` (`release` is a release; anything else, pre-releases and release candidates included, is a snapshot). Optional: bundles written before it was recorded don't have it, and readers then take a version id with `-snapshot-`, `-pre` or `-rc`, or of the weekly form `25w14a`, as a snapshot and any other as a release. It decides only defaults; snapshots are as supported as releases ([ADR 0028](../adr/0028-supported-versions-and-ci.md)). |
| `jar_sha1`     | SHA-1 of the downloaded server jar (the outer bundler jar), lowercase hex, as in the version JSON.                                                                                                                                                                                                                                                                                                                                                                                                          |
| `java_version` | `JAVA_VERSION` from the `release` file of the runtime that ran the data generator.                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `extractor`    | The version of `spyglass-extract` that built the bundle.                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `files`        | Every other file in the bundle, path → SHA-1 of its bytes in lowercase hex, sorted by path (byte order).                                                                                                                                                                                                                                                                                                                                                                                                    |

`bundle.json` contains nothing that varies between runs, such as timestamps or absolute paths. Extracting the same jar with the same extractor gives a byte-identical bundle. The data generator's reports are deterministic (checked for 26.3 on 2026-10-25), and are written with LF line endings on every platform.

## Writing

The extractor writes into a temporary sibling directory and renames it into place only once every file and `bundle.json` are written, so a bundle directory is either complete or absent. An existing bundle is replaced only when asked to.

## Verifying

`spyglass-extract verify <bundle dir>` checks that:

1. `bundle.json` parses and its `format` is known;
2. every file it lists exists and has the listed SHA-1;
3. no other file exists in the bundle directory.

It reports every problem, not only the first.

## Reading

Loaders read the game's JSON as the game's codecs do, including their shorthands and defaults ([ADR 0010](../adr/0010-typed-loading-of-worldgen-json.md)). Each layer's spec lists its types; density functions are in [density-functions.md](density-functions.md#loading). Values that need care:

- **Numbers** are read from their text, as the codec's type asks: a `float` field is the text parsed straight to `f32`, not an `f64` narrowed ([ADR 0011](../adr/0011-parse-json-numbers-from-their-text.md)).

- **Biome parameters.** The reports write each quantized climate value as a float. Loaders recover the game's long with round-to-nearest, not truncation; see [data-sources.md](data-sources.md#climate-parameter-values).

- **Structure templates** (`Bundle::structure_template(id)`: `minecraft:igloo/top` is `data/minecraft/structure/igloo/top.nbt`) are read with `spyglass_data::nbt` ([ADR 0013](../adr/0013-structure-templates-and-nbt.md)). NBT is the game's binary tag format (`NbtIo.readCompressed`):
  - The file is gzip-compressed, and holds one named root tag.
  - Each named tag is a type byte, a name, then the payload. Integers and floats are big-endian.
  - The 12 value types are byte (1), short, int, long, float, double, byte array (7), string (8), list (9), compound (10), int array and long array (12).
  - A list is its element type, an `int` length, then that many payloads. An empty list may give type 0.
  - A compound is named tags up to a type-0 byte. A repeated name keeps the last value.
  - Strings are a `u16` byte length, then Java's modified UTF-8: `U+0000` is `C0 80`, and characters outside the BMP are two 3-byte surrogates.
  - Nesting deeper than 512 is an error, as in the game's `NbtAccounter`.
  - `nbt::read_compressed_limited` caps the decompressed size, for untrusted files such as world saves. `nbt::read_root_int` reads one int child of the root without building the tree, and checks the rest as a full read does, apart from decoding strings.
  - `nbt::write` and `write_compressed` produce the same format (`NbtIo.write`), for tests that build files. A list's element type is its first element's.

  Which fields a template's reader keeps is in [structure-placement.md](structure-placement.md#jigsaw-starts).
