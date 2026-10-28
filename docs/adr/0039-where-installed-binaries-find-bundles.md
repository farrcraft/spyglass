# 0039. Where installed binaries find bundles, and which game versions they trust

- Status: Accepted
- Date: 2026-10-28
- See also: [0026](0026-how-game-versions-coexist.md), [0028](0028-supported-versions-and-ci.md), [0037](0037-versions-and-releases.md)

## Context

The CLI's `--bundles`, the extractor's `--out` and the viewer's first folder all default to `bundles`, relative to the working directory. In a clone run from its root, that's the gitignored `bundles/` folder, and it works. A released binary ([ADR 0037](0037-versions-and-releases.md)) runs from anywhere, though: a downloads folder, a `PATH` entry, a desktop shortcut. A user who extracts a bundle from one folder and runs `spyglass` from another gets "no bundles". The viewer's folder, once chosen, is remembered, but the first run still starts from the wrong place.

A released binary also outlives the game versions it knows. Version switches hold ranges, and the newest are open-ended ([ADR 0026](0026-how-game-versions-coexist.md)). So a build from today, given a bundle for a game version released next year, loads it and answers with today's behavior, with no sign that nobody checked. In a clone that's fine: whoever adds the version runs the oracle ([ADR 0028](0028-supported-versions-and-ci.md)). A user with a downloaded binary has no way to know. Refusing such bundles outright would be safe, but it would make every build useless the day the next snapshot comes out, even though most snapshots change nothing spyglass models.

## Decision

1. **One default bundles folder, chosen by the library** (`spyglass_data::default_bundles_dir()`, re-exported by `spyglass`), in this order:
   1. `SPYGLASS_BUNDLES`, if set;
   2. `./bundles`, if it exists, so a clone keeps working unchanged;
   3. the per-user data folder from `directories::ProjectDirs`, plus `bundles`.

   The CLI, extractor and viewer use it whenever no folder is given, and their help says which folder that is. [bundle-format.md](../specs/bundle-format.md) states the order.

2. **Each build lists the game versions it was checked against,** as `spyglass_data::versions::CHECKED`: the data versions in CI's oracle matrix. A test keeps it equal to the matrix. `spyglass --version` prints it.
3. **Bundles for other versions load with a warning, never an error.** The API returns the warning with the loaded bundle. The CLI prints it to stderr and adds `"unchecked_version"` to `--json`, and the viewer shows a banner. The extractor warns before downloading. The wording distinguishes a version newer than every checked one ("update spyglass") from one between checked versions ("not checked; answers may differ").

## Consequences

- A downloaded binary finds the bundles its own extractor wrote, from any working directory.
- A clone behaves as before, as long as `bundles/` exists. In a clone without it, bundles go to the per-user folder, which may surprise a contributor. `doctor` should say which folder is in use.
- `./bundles` winning when it exists means a user who happens to run spyglass in a folder that has a `bundles` subfolder gets that one. `SPYGLASS_BUNDLES` or `--bundles` overrides it.
- Users of new snapshots keep a working tool and are told plainly that its answers are unchecked.
- The warning is new API surface, and `--json` consumers see a new field. That's additive, so it isn't a breaking change.
- `CHECKED` must change in the same commit that adds a version to CI. The sync test enforces that, and [adding-a-version.md](../adding-a-version.md) gets a step for it.

## Alternatives considered

- **Bundles next to the executable:** natural for a portable zip. But install locations are often not writable (`Program Files`, `/usr/local/bin`), and an upgrade that replaces the folder would delete the bundles.
- **Always the per-user folder:** simplest, but it changes every contributor's workflow and the oracle tests, which look in the repo's `bundles/`.
- **Refusing unchecked versions:** safest for correctness, but every build would stop working for new snapshots within a week.
- **Silently accepting them, as today:** no friction, but it gives no hint that answers may be wrong, which undercuts the project's main promise.
