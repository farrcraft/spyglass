# 0069. A checked crate order, and cited game methods

- Status: Accepted
- Date: 2026-11-11
- See also: [0005](0005-extra-rust-checks.md), whose checks this extends; [0068](0068-float-functions-and-more-clippy-lints.md)

## Context

Two rules were kept by review alone.

- **The crate order.** [architecture.md](../architecture.md#crates) says a crate may depend only on crates listed above it in its table, and the table says what each depends on. Cargo rejects only cycles, so a dependency that runs the wrong way but closes no cycle builds fine. When the check was first run, on 2026-11-11, the table had drifted from the manifests in five places: `spyglass`, `spyglass-feature` and `spyglass-loot` depended on crates their rows didn't name, `spyglass-extract` had no row, and `spyglass-oracle` depended on `spyglass-loot`, which the table listed below it.
- **Which code ports what.** `cargo xtask version-diff --code` lists the game classes a version changes, about 30 for a snapshot and hundreds for a release, but not which spyglass code ports them. Ported functions mostly open their doc comments with the game method they port (``/// `NoiseSpawnFinder.radialSearch`: ...``), about 340 of them, but nothing required it. 26.4-snapshot-8's one change to results, `NoiseBasedChunkGenerator.generateCarvingMask`, was cited as a bare `// generateCarvingMask.`, without its class or backticks.

Horizon checks its layers in CI and maps vanilla methods to its code by the same kind of doc comment ([prior art](../research/prior-art.md)).

## Decision

We will check both:

- **The layer test.** `layers_match_the_architecture`, an xtask unit test that nextest runs on push, reads the crate table's rows and "Depends on" column and `cargo metadata`. A crate's normal and build dependencies on workspace crates must be exactly what its row names, each in a row above its own. `spyglass-oracle` may only be a dev-dependency, and nothing may depend on `xtask`. Dev-dependencies are otherwise free. A failure is fixed in the manifest, or in the table when the layering itself changes, which wants an ADR.
- **Citing the game.** A function that ports a game method opens its doc comment with it in backticks (CLAUDE.md). `version-diff --code` splits each changed class into its changed methods (pairing a removed and an added overload with the same parameter count as a signature change). It reads every backticked `` `Class` `` and `` `Class.method` `` in comments under `crates/`, and lists, for each changed method, the files that cite it and the oracle comparisons in their crates. It ranks them: cited but uncovered by any comparison, cited by name, in a cited class under no cited name, and changed classes nothing cites ([adding-a-version.md](../adding-a-version.md#reading-the-report)).

## Consequences

- The table in architecture.md is now true, and stays true: a new dependency fails the push until its row says so.
- A new version's report says where to look. For 26.4-snapshot-8, the carving mask heads the list, ahead of the `RegistryAccess` to `HolderGetter.Provider` refactor that touched most of the other changed methods.
- **Costs:** a dependency change also edits a Markdown table. The map is only as good as the citations: a stale one points at the wrong code, and a port without one is invisible, so ranks 3 and 4 still need reading. The test runs `cargo metadata`, which takes a fraction of a second. Comments mentioning a game class in passing count as citations, so the file lists for central classes (`ChunkGenerator`) are long.

## Alternatives considered

- **`cargo-deny`'s bans, or a `clippy.toml`-style list, for the layers:** neither knows the table's order, and a second list would drift from architecture.md as the table did.
- **Parsing the manifests with `toml_edit` instead of `cargo metadata`:** no subprocess, but it would have to resolve workspace inheritance and target-specific tables that cargo already resolves.
- **A full ledger, as Horizon's, mapping every game method with structural matching and a verdict map:** far more than spyglass needs to read one version's diff, and its matches need checking anyway (Horizon finds about 1 in 12 wrong). Citations are cheap and exact where they exist.
- **Requiring a citation on every ported function, checked by a lint:** nothing marks a function as a port except the citation itself, so there's nothing to check it against.
