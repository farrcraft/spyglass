# m23. Parity tooling

- Status: Draft
- Milestone: [23. Parity tooling](../roadmap/23-parity-tooling.md)
- Started:
- Completed:

## Goal

Every exit criterion of milestone 23: a layer check that `cargo xtask check` runs, and a `version-diff --code` report that maps changed game methods to the spyglass code that cites them and the oracle comparisons that cover it.

## Approach

### The layer check

An xtask unit test, `layers_match_the_architecture`, in a new `xtask/src/layers.rs`, so nextest runs it on push and in `cargo xtask check`:

1. **The table.** Read [architecture.md](../architecture.md#crates)'s crate table: the crates in row order, and each row's "Depends on" cell as short names (`rng`, `noise`... and `spyglass`, `spyglass-extract` for the front ends). A row's position is its layer.
2. **The manifests.** `cargo metadata --format-version 1 --no-deps`, parsed with `serde_json` (already an xtask dependency): each workspace package's dependencies on workspace crates, by kind.
3. **The rules.** For each crate:
   - its normal (and build) dependencies on workspace crates are exactly the table's cell;
   - each of them is in a row above its own;
   - `spyglass-oracle` appears only as a dev-dependency;
   - `xtask` is a dependency of nothing.
4. **The failure** names the crate, the dependency and the rule, and says which side to fix: the manifest, or the table if the layering itself changed (which wants an ADR).

Dev-dependencies may point anywhere above, as they do today (tests use `spyglass-oracle`'s fixtures). The test reads only files in the repo, so it needs no network and runs offline.

### Changed methods

`code_diff.rs` already splits a class's normalized `javap` text into members (`split`, `declared`). The report gains, per changed class, its methods added, removed and changed (by name and descriptor), each with its line counts, in place of the class's total alone. Constructors and static initializers are methods like any other (`<init>`, `<clinit>`). Lambda bodies (`lambda$name$N`) count towards the method they're named for, since their numbers are normalized away already.

### The code map

A new `xtask/src/citations.rs` builds, from the working tree alone:

- **Citations:** every backticked `` `Class` ``, `` `Class.method` `` and `` `Outer.Inner` `` in a `//`, `///` or `//!` comment of a `.rs` file under `crates/`, as a file and line. A simple name counts when exactly one class under `code_diff::PORTED` has it (`Mth`, `NoiseSpawnFinder`); an ambiguous one counts only with its outer class. About 340 `Class.method` citations exist today.
- **Coverage:** each cited file's crate, mapped to its oracle comparisons through `oracle_test::TESTS`.

`version-diff --code` then prints, for each changed class: the changed methods; for each method, the files that cite it (or the class, when no file names the method); and the oracle comparisons of those files' crates. Then a ranked list:

1. changed methods that spyglass cites in a crate with no oracle comparison: nothing would catch a difference;
2. changed methods that spyglass cites: read these first, and run the comparisons named;
3. changed methods in classes spyglass cites, where no comment names the method: the method may be ported under another name;
4. changed classes under the ported prefixes that nothing cites: probably not ported, listed as leads.

The map is a lead, not a verdict, as Horizon's ledger notes for its own. A citation can be stale, and a port can lack one.

### Citing the game's methods

CLAUDE.md's naming rule already says a name kept from the game is explained where it's defined. It gains one sentence: a function that ports a game method opens its doc comment with it (``/// `NoiseSpawnFinder.radialSearch`: ...``), which is what the code map reads. [adding-a-version.md](../adding-a-version.md)'s reading of the report gains the code map and its ranks.

## Tasks

- [ ] `layers.rs`: read the table, read `cargo metadata`, check the rules, with tests of each rule on small hand-written inputs
- [ ] Run it on the workspace; fix whichever of the table and the manifests is wrong, if either is
- [ ] `code_diff.rs`: changed methods per changed class, with tests on normalized text
- [ ] `citations.rs`: the citation scan and the crate-to-comparisons map, with tests on small source strings (ambiguous names, nested classes, non-comment text ignored)
- [ ] `version-diff --code`: the per-method map and the ranked list
- [ ] Run `version-diff 26.4-snapshot-7 26.4-snapshot-8 --code`; check by eye that the carving mask's bounds rank among the first, and that the ranked list is short enough to read
- [ ] CLAUDE.md: the citation rule; adding-a-version.md: the code map
- [ ] An ADR: the layer check and the citation rule as project policy, extending ADR 0005

## Verification

- xtask unit tests for each layer rule, the method split and the citation scan.
- A deliberate bad dependency (`spyglass-noise` on `spyglass-biome`) fails the layer test, in a scratch branch.
- The snapshot-7 to snapshot-8 report, read by eye against what that snapshot is known to change ([adding-a-version.md](../adding-a-version.md#cost-log)).

Nothing about worldgen changes, so `cargo xtask oracle-test` isn't needed.

## Durable outputs

- [ ] The ADR on the layer check and the citation rule
- [ ] [architecture.md](../architecture.md#crates): the rule says it's checked
- [ ] [adding-a-version.md](../adding-a-version.md): the code map and its ranks
- [ ] CLAUDE.md: citing the game's methods

## Notes
