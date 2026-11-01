# m12. First run

- Status: Completed
- Milestone: [12. First run](../../roadmap/completed/12-first-run.md)
- Started: 2026-10-29
- Completed: 2026-10-29

## Goal

A downloaded viewer gets its first bundle without a terminal, into a folder beside itself that the other binaries also find. Covers every exit criterion of milestone 12.

## Approach

- **Default folder** ([ADR 0043](../../adr/0043-bundles-beside-the-executables.md), [bundle-format.md](../../specs/bundle-format.md#where-bundles-are)): `spyglass_data::bundle` gains `app_bundles_dir()` (`bundles` beside `current_exe`) and `user_bundles_dir()`, and `choose_bundles_dir` takes whether the app folder exists. `spyglass` re-exports both.
- **Extraction in the viewer** ([ADR 0042](../../adr/0042-the-viewer-extracts-bundles-itself.md)): `spyglass-viewer` depends on `spyglass-extract`. A new `extract` module in the viewer holds the background work: fetching the manifest, listing the versions from the newest down to the oldest checked one (releases and snapshots), and running `extract::extract` on a thread that shares the step under way and sends the outcome over a channel. `app/saves.rs`'s no-bundle screen becomes the folder choice, the version picker and the Extract button. The World section gets the same picker, collapsed, to add versions.
- **Java advice in one place:** the "install Java N" text moves from `spyglass-extract`'s `main.rs` to a method on `JavaNotFound`, used by the command and the viewer.
- **No console for `java`:** `datagen::run_reports` sets `CREATE_NO_WINDOW` on Windows.
- **No console for the viewer:** `windows_subsystem = "windows"` in release builds.

## Tasks

- [x] Release viewer builds use the Windows GUI subsystem.
- [x] `app_bundles_dir`, `user_bundles_dir` and the new default order, with tests of `choose_bundles_dir`.
- [x] `JavaNotFound::advice`, used by `spyglass-extract`.
- [x] `CREATE_NO_WINDOW` for the data generator on Windows.
- [x] The viewer's extraction: folder choice, version picker, progress, errors, and opening the new bundle.
- [x] Docs: bundle-format.md, architecture.md, the archive's GETTING-STARTED.txt, README, CHANGELOG.
- [x] Check by hand on Windows: a release build in a new folder, with and without Java.

## Verification

- Unit tests for the folder order and the version list.
- `cargo xtask check`.
- By hand: the first exit criterion, on Windows, from a `cargo xtask dist` archive.

## Durable outputs

- [x] [ADR 0042](../../adr/0042-the-viewer-extracts-bundles-itself.md) and [ADR 0043](../../adr/0043-bundles-beside-the-executables.md).
- [x] [bundle-format.md](../../specs/bundle-format.md#where-bundles-are): the new order.
- [x] [architecture.md](../../architecture.md): the viewer uses `spyglass-extract`.

## Notes

- 2026-10-29: completed. The owner tested a `cargo xtask dist` build of 1ea2a2c on Windows.
- 2026-10-29: the viewer creates the bundles folder only when an extraction starts. If that fails (an unwritable folder, as under Program Files), it says so and offers the per-user folder, rather than probing for writability up front.
- 2026-10-29: the folder beside the executable counts only once it exists, so the CLI on a `PATH` never tries to write there and existing per-user bundles are still found.
