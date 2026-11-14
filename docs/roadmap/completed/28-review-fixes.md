# 28. Fixes from the 2026-11-13 review

- Status: Completed
- Depends on: [26](26-data-packs.md)
- Plans: [m28-review-fixes](../../plans/completed/m28-review-fixes.md)

## Goal

Close the findings of the code review of milestones 23 to 26 (Java's `Math`, what a save knows, data packs, parity tooling) before more work builds on them. `spyglass-math` must keep its "correctly rounded" claim. A save's answers must be certain only where the game's would be. A data pack must load as the game loads it and must not be able to exhaust memory. The code these milestones added must be split where it grew too long.

## Scope

- Parity: `pow` at exact midpoints and huge exponents, trig reduction for large arguments, structure starts in chunks saved by 1.18.0 and 1.18.1, and pack metadata, paths and tags read as the game reads them.
- Untrusted input: gzip bombs in pack templates, and duplicate packs in `level.dat`.
- The certainty of saved answers, and the save-chunk cache.
- Tooling and CI: stray oracle variants, the triggers and literals in the math and release workflows, and logic repeated in xtask.
- Documentation that no longer matches the code.
- File and function length: the viewer's map, `map_features`, `pack.rs` and three xtask modules.

Not in scope: new behavior beyond what a finding needs.

## Exit criteria

- [x] Every finding in the plan is fixed, or closed with a recorded reason.
- [x] Every parity and bug fix has a test that fails without it.
- [x] No function the plan names is longer than about 80 lines, and no file it names is longer than about 800 non-test lines.
- [x] `cargo xtask check` and `cargo xtask oracle-test` pass for every supported version, and `cargo xtask math-report` shows no new open or wrong cases.
