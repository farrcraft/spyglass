# 0043. Bundles beside the executables

- Status: Accepted
- Date: 2026-10-29
- Amends: point 1 of [0039](0039-where-installed-binaries-find-bundles.md), the default bundles folder
- See also: [0042](0042-the-viewer-extracts-bundles-itself.md)

## Context

[ADR 0039](0039-where-installed-binaries-find-bundles.md) picks the default bundles folder as `SPYGLASS_BUNDLES`, then `./bundles` if it exists, then a per-user data folder (`%LOCALAPPDATA%\spyglass\data\bundles` on Windows). It rejected bundles next to the executable because install folders such as `Program Files` and `/usr/local/bin` are often not writable, and because an upgrade that replaces the folder would delete the bundles.

spyglass ships as a zip of three binaries with no installer ([ADR 0038](0038-how-release-archives-are-built.md)). People who unpack a zip into a folder of their own expect what the program writes to stay in that folder, where they can see it, move it and delete it together with the program. A hidden per-user folder surprises them. The working-directory rule doesn't help either: a double-clicked viewer's working directory is wherever the shell chose, and the three binaries are run from different places.

## Decision

We will add `bundles` in the folder of the running executable to the default order, after `./bundles` and before the per-user folder, and only if it exists:

1. `SPYGLASS_BUNDLES`, if set;
2. `./bundles`, if it exists;
3. `<folder of the running executable>/bundles`, if it exists;
4. the per-user data folder, plus `bundles`.

The library names the two candidate folders (`app_bundles_dir`, `user_bundles_dir`) beside `default_bundles_dir`. The viewer, when it finds no bundles, offers three places to extract to: next to spyglass (preselected), the per-user folder, or a folder its user picks. When the folder next to spyglass can't be created, it says so and offers the per-user folder.

## Consequences

- One unpacked release folder is self-contained: the viewer extracts next to itself, and the CLI and extractor from the same folder find those bundles from any working directory.
- Nothing changes for an existing user: the new folder only counts once it exists, so bundles already in the per-user folder are still found.
- A user who upgrades by deleting the old folder deletes its bundles. They're cheap to rebuild (the jar cache stays in the per-user cache folder), and the folder is in plain sight, which is what the user chose.
- A program run from an unwritable location never gets the folder created, so it falls through to the per-user folder as before, except in the viewer, which says why.
- Development builds live in `target/<profile>/`, so a contributor without `bundles/` in the clone who extracts "next to spyglass" from the viewer writes into `target/`, where `cargo clean` deletes it. A clone's `bundles/` still wins over it.
- The default now depends on `std::env::current_exe`, which can fail or, on Linux, be a path through a symlink. A failure skips the rule.

## Alternatives considered

- **Next to the executable, always (created on demand):** the CLI on a `PATH` in `/usr/local/bin` would try to write there. The existence check keeps the rule opt-in.
- **Viewer only:** the viewer remembers its folder already, but bundles it extracted next to itself wouldn't be found by `spyglass locate` without `--bundles`.
- **A portable-mode marker file:** clearer intent, but one more thing to ship and explain. An existing `bundles` folder is the marker.
