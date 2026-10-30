# 17. Markers and saved seeds

- Status: Planned
- Depends on: [13](completed/13-viewer-redesign.md)
- Plans: none yet

## Goal

The viewer remembers what you've found and done in a world: your own markers, which structures you've already visited, and the seeds you care about. A view can be passed to someone else as text. All of it is stored locally and is yours to back up or move.

## Scope

- **Markers:** right-click (or a key) places a pin, which can be named, coloured and saved. Markers belong to a world (a seed and version, or a save) and a dimension, show on the map and in a list, and can be edited or deleted.
- **Done:** any structure marker, locate pin or search match can be marked done. Done markers are drawn dimmed, and a layer toggle hides them.
- **Saved seeds:** recent seeds kept automatically, saved ones named, and a list to reopen either, with each seed's markers and its last view.
- **Sharing a view:** copy the seed, version, dimension, view and layers as a short text, and paste one to open it. No server is involved. The text's format is versioned, so old texts keep opening.
- **Storage:** one file per user beside the viewer's settings, in a documented format, with import and export. A new spec covers the file and the shared text. An ADR covers where user data lives, and why the library doesn't hold it, so the CLI and scripts can read it from the spec.
- **Exports:** the image export can include markers.

Not in scope:

- Syncing between machines, or any account. Spyglass stays local only.
- Markers written into a world save.

## Exit criteria

- [ ] Markers, done states and saved seeds survive a restart, a version switch and the same file imported on another machine.
- [ ] A copied view text opens the same view in a fresh viewer, and texts from the first version of the format still open after a change to it (a test).
- [ ] A corrupt or hostile data file loads what it can and reports the rest, without a panic (fuzzed, or with hand-made cases).
- [ ] `cargo xtask check` passes.
