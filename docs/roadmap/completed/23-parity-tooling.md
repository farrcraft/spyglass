# 23. Parity tooling: checked layers, and version diffs mapped to the code

- Status: Completed
- Depends on: none
- Plans: [m23-parity-tooling](../../plans/completed/m23-parity-tooling.md)

## Goal

Two rules that review keeps by hand become checks. First, crates depend only on the crates [architecture.md](../../architecture.md#crates) lists above them, and its table says what each depends on. Second, when a version changes the game's code, `cargo xtask version-diff --code` says which spyglass code ports what changed, and which oracle comparisons cover it, so adding a version starts from a list of files rather than a list of classes. Horizon's port ledger and layer check are the precedent ([prior art](../../research/prior-art.md)).

## Scope

- **The layer check:** a test that reads the crate table's order and "Depends on" column and compares them with `cargo metadata`. A crate's normal dependencies must be workspace crates listed above it, and exactly the ones the table names. `spyglass-oracle` is only ever a dev-dependency.
- **Changed methods:** `version-diff --code` splits each changed class into methods, and lists the methods added, removed and changed, with line counts.
- **The code map:** for each changed class and method, the spyglass files whose comments name it (`` `Class` `` or `` `Class.method` ``, the existing house style), and the oracle comparisons in those files' crates.
- **The ranking:** changed code that spyglass cites but no oracle comparison covers comes first; changed classes under the ported prefixes that nothing cites come last, as leads.

Not in scope:

- Item-level reachability, as Horizon's harness selection does. spyglass's crates are small enough that a crate is the unit.
- Filing issues from the report.

## Exit criteria

- [x] `cargo xtask check` fails when a crate depends on one below it, or when the table and the manifests disagree.
- [x] `version-diff --code` between 26.4-snapshot-7 and 26.4-snapshot-8 lists the changed methods, the files that cite them, and the oracle comparisons that cover them, and the report names the carving mask's bounds, the one change that snapshot made to results.
- [x] [adding-a-version.md](../../adding-a-version.md) reads the code map, and the codebase's rule on citing the game's methods is written down where contributors see it.
