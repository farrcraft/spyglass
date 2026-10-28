# 10. Hardening

- Status: Completed
- Depends on: [09](09-fast-density.md)
- Plans: [m10-hardening](../../plans/completed/m10-hardening.md)

## Goal

Close the findings of the repo audit of 2026-10-28 before new worldgen layers build on the code. A hostile save or data pack must not take the process down, every long operation must stop when asked, the docs must match the code, and a new Rust release must not turn CI red.

## Scope

- Untrusted input: world saves (NBT, region files) and data packs (density functions, jigsaw pools, codec numbers).
- Responsiveness: stopping search and long locates, the viewer's UI thread and worker threads, CLI panics on user input.
- Tooling and supply chain: the oracle's EULA step, the toolchain pin, download verification, setup scripts, CI coverage.
- Documentation accuracy, broken links, and a check that keeps links from breaking again.
- Maintainability debt the audit found: the viewer's `app.rs`, code repeated between evaluator families, public types that can't grow.
- The test gaps behind each of these.

Not in scope: new worldgen behavior. Surface rules, features and the final spawn block stay deferred; this milestone only records them in the roadmap.

## Exit criteria

- [x] Every finding in the plan is fixed, or closed with a recorded reason.
- [x] No input file in a save or pack makes the process abort, as shown by tests for each finding and by fuzzing the NBT and region readers.
- [x] Ctrl-C, `--time` and the viewer's Stop end a search within one second of the request, measured on a stronghold query with `--top 1000`.
- [x] `cargo xtask check` reports broken relative links and anchors in Markdown, and passes.
- [x] `cargo xtask check` and `cargo xtask oracle-test` pass for every supported version.
