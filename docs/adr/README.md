# Architecture Decision Records

Each ADR records one significant decision: the context, what was decided, and the consequences. Accepted ADRs aren't rewritten. A changed decision gets a new ADR that supersedes the old one.

Start from [template.md](template.md). Files are named `NNNN-kebab-case-title.md`. In Claude Code, the `/adr` skill handles numbering, the template and this index.

| #                                                             | Title                                                                 | Status                                                               |
| ------------------------------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------- |
| [0001](0001-record-architecture-decisions.md)                 | Record architecture decisions                                         | Accepted                                                             |
| [0002](0002-git-hooks-for-local-checks.md)                    | Git hooks for local checks                                            | Accepted                                                             |
| [0003](0003-cargo-xtask-for-project-commands.md)              | cargo xtask for project commands                                      | Accepted                                                             |
| [0004](0004-oracle-standalone-harness.md)                     | Oracle: standalone Java harness on the server jar                     | Accepted                                                             |
| [0005](0005-extra-rust-checks.md)                             | Extra Rust checks: lints, dependencies, MSRV and spelling             | Accepted                                                             |
| [0006](0006-bundle-format-raw-json-tree.md)                   | Version bundle format: raw JSON tree with a manifest                  | Accepted                                                             |
| [0007](0007-dev-container-and-linux-check.md)                 | Dev container and a containerized Linux check                         | Accepted                                                             |
| [0008](0008-game-derived-test-values-stay-local.md)           | Game-derived test values stay local                                   | Accepted                                                             |
| [0009](0009-md5-from-the-md-5-crate.md)                       | MD5 from the md-5 crate                                               | Accepted                                                             |
| [0010](0010-typed-loading-of-worldgen-json.md)                | Typed loading of worldgen JSON                                        | Accepted                                                             |
| [0011](0011-parse-json-numbers-from-their-text.md)            | Parse JSON numbers from their text                                    | Accepted                                                             |
| [0012](0012-java-trig-results-that-depend-on-the-platform.md) | Java trig results that depend on the platform                         | Accepted                                                             |
| [0013](0013-structure-templates-and-nbt.md)                   | Structure templates in bundles, read with our own NBT reader          | Accepted                                                             |
| [0014](0014-public-api-world-and-session.md)                  | Public API: a loaded world and seed-bound sessions                    | Accepted                                                             |
| [0015](0015-locate-checks-on-a-real-server.md)                | Check `/locate` answers on a real dedicated server                    | Accepted                                                             |
| [0016](0016-extractor-crate-and-read-only-data.md)            | The extractor in its own crate; `spyglass-data` reads only            | Accepted                                                             |
| [0017](0017-map-rendering-in-the-public-api.md)               | Map rendering in the public API                                       | Accepted                                                             |
| [0018](0018-biome-palette.md)                                 | A biome palette of spyglass's own                                     | Accepted                                                             |
| [0019](0019-gui-stack-and-msrv-1-95.md)                       | The viewer's GUI stack, and MSRV 1.95                                 | Accepted                                                             |
| [0020](0020-search-query-language.md)                         | A line-oriented query language for seed search                        | Accepted                                                             |
| [0021](0021-search-engine.md)                                 | The seed search engine: staged checks on rayon, deterministic ranking | Accepted                                                             |
| [0022](0022-terrain-crate-and-volume-evaluation.md)           | Terrain in its own crate, with the game's volume evaluation           | Accepted                                                             |
| [0023](0023-dimensions-in-the-public-api.md)                  | Dimensions in the public API                                          | Accepted                                                             |
| [0024](0024-dimensions-in-the-search-language.md)             | Dimensions in the search language                                     | Superseded by [0025](0025-inferring-dimensions-in-search-queries.md) |
| [0025](0025-inferring-dimensions-in-search-queries.md)        | Inferring dimensions in search queries                                | Accepted                                                             |
| [0026](0026-how-game-versions-coexist.md)                     | How game versions coexist                                             | Proposed                                                             |
| [0027](0027-oracle-harness-across-versions.md)                | The oracle harness across versions                                    | Proposed                                                             |
| [0028](0028-supported-versions-and-ci.md)                     | Supported versions, snapshots and CI                                  | Proposed                                                             |
