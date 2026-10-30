# 0048. The viewer keeps user data in a file of its own

- Status: Accepted
- Date: 2026-10-29

## Context

The viewer is gaining data that its user makes and can't make again: markers they place, structures they've marked done, and named seeds with each one's last view. Until now it remembered only preferences, in eframe's `app.ron` under the key `spyglass` (`%APPDATA%\spyglass\data\` on Windows, the file `SPYGLASS_VIEWER_SETTINGS` names when set). Losing preferences costs a few clicks. Losing a world's markers costs hours, so this data needs more care.

- **It must survive:** restarts, crashes mid-write, game version switches, a newer viewer and an older one sharing the file, and two viewers open at once.
- **It's portable:** a user should be able to back it up, move it to another machine, and read it with a script.
- **The library stays stateless:** `spyglass` answers about worlds and keeps nothing between calls ([ADR 0014](0014-public-api-world-and-session.md)). The CLI is a front end over it with no per-user state at all.
- **Spyglass is local only:** no accounts and no server.

The same milestone adds a short text that opens a view in someone else's viewer. It's a second format that must keep working as it changes.

## Decision

We will keep user data in one JSON file per user, `spyglass-data.json`, beside the viewer's settings, in a format that [user-data.md](../specs/user-data.md) defines and versions. The viewer owns it, and the library and CLI don't read it.

- **Where:** the folder of eframe's settings file, which is `eframe::storage_dir("spyglass")`, or the folder of `SPYGLASS_VIEWER_SETTINGS` when that's set. So `run-viewer` and `cargo xtask viewer-timing` never touch a user's markers.
- **The format:** JSON, read and written by hand from `serde_json::Value`, not derived. This gives exact error messages, keeps fields the viewer doesn't know, and doesn't depend on serde's `flatten` working with `arbitrary_precision`. The spec, not a Rust type, is the contract, so scripts in any language can read the file.
- **A world** is a seed, plus a save's level name when it came from a save. The key holds no game version and no path:
  - A marker is a place, and places don't move between versions, so switching versions keeps a world's markers.
  - A save's path changes between machines; its seed and name don't.
- **Reading is lenient:** each world, marker and done place is decoded on its own. A bad one is skipped and reported, and the rest load. Limits on size and counts stop a hostile file from doing more than that.
- **Writing:**
  - The viewer writes a temporary file, then renames it over the old one.
  - The first write of each run keeps the old file as `spyglass-data.json.bak`.
  - Before writing, the viewer re-reads the file. If another viewer changed it, the two are merged by the import rules, so neither loses the other's markers.
  - Deletions are kept as tombstones, so a merge can't bring back what was deleted.
- **Format versions:** a file whose `format` is newer than the viewer knows is read for what it can, but never written over.
- **The view text** (`spyglass:1;s=…;…`) is plain `key=value` fields with a format number. Every text the viewer has written must keep opening, which frozen test fixtures check.

## Consequences

- The data can be backed up, moved, merged and scripted, and the spec says what each field means.
- Decoding and encoding by hand are more code than a serde derive, and the spec and the code must be kept in step by hand.
- The CLI can't list a user's markers. Anyone who wants that reads the file from the spec. A shared crate can come later if a second front end needs it.
- Tombstones make the file grow with deletions. The per-world limits bound it, and a world that's forgotten takes its tombstones with it.
- Merging on write closes the usual window for losing data between two viewers, but not all of it: two writes in the same instant can still race. There's no lock.
- A user who keeps one world under two names (a save and its bare seed) sees two worlds. Saves are keyed by name so that a save is its own world even when a seed is shared.

## Alternatives considered

- **eframe's storage (`app.ron`):** its format is eframe's, not ours. It's written whole on eframe's schedule, has no import or merge, and settings and data would share one fate.
- **A `spyglass-userdata` crate that the CLI shares:** the CLI has no use for it yet. A crate would make the format a Rust API as well as a file, with two things to keep stable instead of one.
- **SQLite:** robust to concurrent writers, but it's a C dependency for a few kilobytes of data, it's harder to read by hand, and a database is harder to send to someone than a file.
- **Keying worlds by seed and version:** the milestone's first wording. It hides every marker when a played world updates to a new version.
- **Keying saves by path:** breaks when the save moves, and between machines.
- **A file lock between viewers:** locks behave differently across platforms and can be left stale after a crash. Merging handles the common case, two windows open on one machine.
- **Base64 view texts:** shorter, but opaque. A readable text shows what it opens, and still works after an extra space or a mangled character.
