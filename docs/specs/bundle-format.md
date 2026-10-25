# Version bundle format

Status: draft. The format is decided in [ADR 0006](../adr/0006-bundle-format-raw-json-tree.md).

A version bundle holds every worldgen parameter spyglass needs for one game version. `spyglass-extract` builds it locally from the user's own server jar ([data-sources.md](data-sources.md)). Bundles are never committed: they live in `/bundles`, which is gitignored.

## Layout

A bundle is a directory, by default `bundles/<version id>/`:

| Path                                  | From                                                                         | 26.3 files |
| ------------------------------------- | ---------------------------------------------------------------------------- | ---------- |
| `bundle.json`                         | The extractor (below).                                                       | 1          |
| `data/minecraft/worldgen/**`          | The inner game jar, same path.                                               | 1,144      |
| `data/minecraft/tags/worldgen/**`     | The inner game jar, same path.                                               | 123        |
| `data/minecraft/dimension_type/**`    | The inner game jar, same path.                                               | 4          |
| `reports/biome_parameters/minecraft/` | The data generator's `--reports` output: `overworld.json` and `nether.json`. | 2          |

Rules:

- Files are copied **byte for byte**. Nothing is reformatted, normalized or converted.
- Every file under the included directories is copied, whatever its extension.
- Paths use `/` and are relative to the bundle root, both on disk and in `bundle.json`.
- Nothing else is in the bundle directory. A file that isn't listed in `bundle.json` makes the bundle invalid.

The set of included directories is part of the format. Adding one (structure templates under `data/minecraft/structure/` are expected later) changes this spec and the extractor's version, but not `format`.

## `bundle.json`

UTF-8 JSON, pretty-printed with two-space indent, keys in the order below, and a trailing newline:

```json
{
  "format": 1,
  "version": "26.3",
  "data_version": 5023,
  "jar_sha1": "33680f5f2ac32864d6d7cf5e56a705fdb3e05f4c",
  "java_version": "25.0.3",
  "extractor": "0.1.0",
  "files": {
    "data/minecraft/dimension_type/overworld.json": "…",
    "reports/biome_parameters/minecraft/overworld.json": "ed8590a657f04acb51074c2082d2b97c81a56f70"
  }
}
```

| Field          | Meaning                                                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `format`       | Bundle format version, an integer. This spec describes `1`. Readers reject formats they don't know.                      |
| `version`      | The game version id, as in the version manifest.                                                                         |
| `data_version` | `world_version` from the inner jar's `version.json`. World saves record the same number in `level.dat` as `DataVersion`. |
| `jar_sha1`     | SHA-1 of the downloaded server jar (the outer bundler jar), lowercase hex, as in the version JSON.                       |
| `java_version` | `JAVA_VERSION` from the `release` file of the runtime that ran the data generator.                                       |
| `extractor`    | The version of `spyglass-data` that built the bundle.                                                                    |
| `files`        | Every other file in the bundle, path → SHA-1 of its bytes in lowercase hex, sorted by path (byte order).                 |

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

Loaders read the game's JSON as the game's codecs do, including their shorthands. Values that need care:

- **Biome parameters.** The reports write each quantized climate value as a float. Loaders recover the game's long with round-to-nearest, not truncation; see [data-sources.md](data-sources.md#climate-parameter-values).
