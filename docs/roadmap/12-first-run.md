# 12. First run

- Status: In progress
- Depends on: [11](11-releases.md)
- Plans: [m12-first-run](../plans/m12-first-run.md)

## Goal

Someone who downloads spyglass and double-clicks the viewer gets from an empty window to a map without opening a terminal, and finds the game data where they'd expect it: in the folder they unpacked spyglass into.

## Scope

- The release viewer opens no console window on Windows.
- Bundles in a `bundles` folder beside the executables count as a default location for all three binaries ([ADR 0043](../adr/0043-bundles-beside-the-executables.md)).
- The viewer extracts bundles itself: a choice of folder, a game version picker and an Extract button with progress ([ADR 0042](../adr/0042-the-viewer-extracts-bundles-itself.md)). It works both on the first run and to add versions later.
- The viewer's first-run text never mentions `cargo`.
- The first-run guide in the release archive and the docs that describe where bundles are.

Not in scope:

- Downloading or installing Java from the viewer. It says what to install, as the extractor does.
- Cancelling an extraction.
- An installer, or a settings screen beyond the bundles folder.

## Exit criteria

- [ ] A release build of the viewer, unpacked into a new folder on Windows with Java installed, extracts a bundle into `bundles` beside itself and shows a map, with no console window and no terminal.
- [ ] `spyglass` and `spyglass-extract` from the same folder find that bundle from any working directory.
- [ ] With no Java runtime, the viewer says which version to install and how, as the extractor does.
- [ ] `cargo xtask check` passes.
