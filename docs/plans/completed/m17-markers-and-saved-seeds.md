# m17. Markers and saved seeds

- Status: Completed
- Milestone: [17. Markers and saved seeds](../../roadmap/completed/17-markers-and-saved-seeds.md)
- Started: 2026-10-29
- Completed: 2026-10-29

## Goal

The viewer keeps what its user finds and does in a world. That covers markers they place, done states on structures, locate pins and search matches, and recent and saved seeds with each one's last view. A view can be copied as a short text and pasted into another viewer. All of it lives in one documented file per user, which can be imported and exported, and image exports can include the markers. This plan covers all four of the milestone's exit criteria.

## Approach

Today the viewer remembers only its settings, in eframe's `app.ron` (`%APPDATA%\spyglass\data\` on Windows), or in the file `SPYGLASS_VIEWER_SETTINGS` names. Settings are preferences and can be thrown away. A user's markers can't. So the new data gets a file of its own, in a format the project defines, specifies and versions, and doesn't go in eframe's storage.

**Where it lives** (a new ADR before any code; this is the proposal, the ADR settles it).

- **The file:** `spyglass-data.json`, in the same folder as the settings file. When `SPYGLASS_VIEWER_SETTINGS` is set, the folder is that file's, so `run-viewer` and `viewer-timing` never touch the user's own markers.
- **The format:** JSON through `serde_json`, which is already a workspace dependency, so no new licence. JSON over RON, because scripts in any language can read it. The spec, not a Rust type, is the contract.
- **Who owns it:** the viewer. The library stays stateless: it answers about worlds and keeps nothing between calls ([ADR 0014](../../adr/0014-public-api-world-and-session.md)). The CLI doesn't read the file. Anyone who wants the data reads it as the spec describes. The ADR also covers the alternatives:
  - a `spyglass-userdata` crate that the CLI could share;
  - eframe's storage;
  - a SQLite file.

**What the file holds** (the spec settles names and limits).

```json
{
  "format": 1,
  "worlds": [
    {
      "world": { "seed": "-5", "save": null },
      "name": "Speedrun practice",
      "saved": true,
      "opened": "2026-10-29T18:00:00Z",
      "version": "26.3",
      "dimension": "minecraft:overworld",
      "views": {
        "minecraft:overworld": {
          "centre": [100.5, -200.5],
          "scale": 8,
          "y": 64
        }
      },
      "markers": [
        {
          "id": "m-…",
          "dimension": "minecraft:overworld",
          "x": 120,
          "y": null,
          "z": -310,
          "name": "Base",
          "colour": "#e0a040",
          "done": false,
          "modified": "…"
        }
      ],
      "done": [
        {
          "dimension": "minecraft:overworld",
          "kind": "structure",
          "what": "minecraft:village_plains",
          "x": 128,
          "z": -320,
          "modified": "…"
        }
      ]
    }
  ]
}
```

- **A world** is a seed, plus a save's name when it came from a save. The key holds no game version and no path:
  - A version switch keeps the world's markers on screen, which is what the first exit criterion asks.
  - A save's path changes between machines, and its level name and seed don't.
  - The world records the version it was last opened in. A marker doesn't need a version: it's a place, and places don't move between versions.
  - The ADR records "seed and version" as the alternative. It would hide the pins after every update of a world the user plays.
- **Seeds are strings** in the file. An `i64` isn't exact as a number in every JSON reader.
- **Markers** have an id, a dimension, a block x and z (and a y, optional), a name, a colour from a fixed palette of eight, and a modified time.
- **Done** is a set of places, not a flag on markers, so it can cover things the file doesn't otherwise hold:
  - a structure start, as its structure id and start position;
  - a locate pin or search match, as its target id and position;
  - a user marker, as its own `done` field.

  Recomputed starts land on the same key, so a done structure stays done through a restart and a version switch.

- **Recent and saved seeds:**
  - Every world the viewer opens is recent, kept by `opened`. Unsaved worlds past the newest 20 are dropped, unless they hold markers or done places.
  - Naming a world saves it.

**Reading and writing.**

- **Lenient reading:** the file is read as a `serde_json::Value`, then each world, marker and done place is decoded on its own. A bad entry is skipped and reported with its path in the file, as in `worlds[3].markers[12]: colour "#zz0000" is not a colour`.
- **Unknown fields** are kept, so a newer viewer's file round-trips through an older one without losing them.
- **Limits:** on file size (16 MiB), on worlds, markers per world and string lengths, and finite coordinates within the world border. Anything beyond is skipped and reported, never a panic.
- **Format versions:** a `format` newer than the viewer knows opens read-only. The viewer says so and doesn't overwrite it.
- **Problems** show once, in a notice in the status bar with a details list.
- **Writing:** the file is written to a temporary file in the same folder, then renamed over the old one. That happens on a change, at most once a second, and on exit. The old file is kept as `spyglass-data.json.bak` once per run.
- **Two viewers at once:** before writing, the viewer re-reads the file and merges it with its own data, so the last writer doesn't drop the other's markers. The ADR records why a lock wasn't chosen.

**Import and export.**

- **Export** writes the whole file, or one world, to a chosen path.
- **Import** merges: worlds by key, markers by id, done places by key, and the later `modified` wins. Importing a file twice changes nothing, which a test checks.

**The shared view text.**

- It holds the seed, version, dimension, centre, scale, y and layers: the structures, highlighted biomes and the layer toggles. Markers aren't in it.
- **A proposal:** a prefix with the format version, then `key=value` fields separated by `;`, readable and safe in chat. For example, `spyglass:1;s=-5;v=26.3;d=nether;c=100,-200;z=8;y=64;st=village_plains,stronghold;l=spawn,confirm`. Ids drop the `minecraft:` namespace when it's that.
- **Compatibility rules:**
  - Unknown keys are ignored.
  - Missing keys take the viewer's defaults.
  - Every version of the text this viewer has ever written still parses.
  - The test fixtures keep one text per format version, and never change them.
- **Saves:** a view from a save is shared as its seed and version. The path means nothing on another machine.
- **Opening one:**
  - A "Copy view" button in the toolbar and on the map's context menu.
  - "Open view…" in the toolbar takes a pasted text.
  - Pasting a view text onto the map (Ctrl+V, when no text field has focus) opens it too.
  - If the bundle for its version isn't there, the viewer opens it in the nearest bundle and says so, as for a save.

**The viewer.**

- **Placing a marker:** the map's context menu gains "Add marker here", and M adds one at the pointer. A new marker opens its editor: name, colour, delete.
- **Drawing:** markers are drawn as pins in their colour, with the name on hover, and at full size with the name beside them up close. Picking and hovering are the same as for structure icons (`markers.rs`).
- **Done:**
  - The hover card and context menu of a structure icon, locate pin, search match or marker gain "Mark done" and "Mark not done".
  - Done things are drawn dimmed, at about 40% opacity.
  - A "Done" toggle in the layers hides them instead.
- **A Markers tab** in the side panel lists the world's markers in the shown dimension, then the done places. Each row can be centred, renamed, recoloured, marked done or deleted.
- **Worlds:** the seed field gains a dropdown of saved worlds by name, then recent ones, each showing its marker count. Opening one restores its version, dimension and last view. A saved world can be renamed or forgotten.

**Exports with markers.** The image export is drawn by the library ([render.md](../../specs/render.md#layers)), and the viewer shows the matching `spyglass render` command. So markers go into the image through the library too, as a generic layer with no names and no file:

- `Layers` gains `pins: Vec<Pin>`, where a pin is a block position and an RGB colour. They're drawn last, after the spawn, as a fixed-size shape in render.md, in their given order. They aren't drawn in an area they don't touch, by the 5-pixel rule markers already follow.
- `Session::map_features` reports pins as well.
- `spyglass render` gains `--pin x,z[,#rrggbb]`, repeatable. The viewer's command lists one per marker in the area.
- A checkbox in the export menu, "Include markers", is on by default.
- Names and done states aren't drawn: images have no text yet, and adding a font to the library is a bigger change than this milestone needs. The Notes record it.

## Tasks

- [x] **ADR:** where user data lives. It covers:
  - the file beside the settings, and `SPYGLASS_VIEWER_SETTINGS`;
  - JSON, owned by the viewer, and why the library and CLI don't hold it;
  - the world key without a version or path;
  - merging on write between two viewers;
  - the alternatives above.
- [x] **Spec:** `docs/specs/user-data.md`. It covers:
  - the file: fields, limits, lenient reading, unknown fields, format versions, merge rules, backup;
  - the view text: grammar, defaults, compatibility rules, and how a save's view is shared.

  Add it to the specs index.

- [x] **Model and file** (a `userdata` module in the viewer). It covers:
  - the types;
  - lenient reading with reported problems;
  - limits;
  - atomic writing and the backup;
  - merging with the file on disk before writing;
  - import, export and merge.

  Tests:
  - A round trip.
  - Unknown fields kept.
  - A newer format left read-only.
  - Importing twice is a no-op.
  - Two writers' markers both kept.

- [x] **Hostile files:** hand-made cases, and a mutation test in the style of `mutated_queries_never_panic`. Each case loads what it can and reports the rest, without a panic. The cases include:
  - truncated JSON;
  - wrong types;
  - huge numbers, NaN as a string, and coordinates past the border;
  - 10⁶ markers;
  - deep nesting;
  - duplicate ids;
  - invalid UTF-8;
  - a file over the size limit;
  - a missing file, and an empty one.

  A cargo-fuzz target would need the reader in a library crate (ADR 0036's targets are the library's readers), so this stays a test unless the ADR moves it.

- [x] **View text** (a `share` module): encode, decode and defaults. Tests:
  - A round trip of every field.
  - Unknown keys ignored.
  - A save's view shared as its seed.
  - The format-1 fixture texts open to the expected views. These are frozen.
- [x] **Library pins:** `Pin` and `Layers::pins` in `spyglass`, drawn and reported as render.md will say. Tests:
  - A pin's pixels.
  - Order and the edge rule.
  - `map_features` agrees with the image.
- [x] **CLI:** `spyglass render --pin`, its parsing and errors, and a CLI test.
- [x] **Viewer: markers:**
  - add from the context menu and with M;
  - the editor;
  - drawing and picking;
  - the Markers tab;
  - storage per world and dimension.
- [x] **Viewer: done:** marking from structure icons, locate pins, search matches and markers, the dimmed drawing, and the Done layer toggle.
- [x] **Viewer: worlds:** recent worlds kept automatically; naming, renaming and forgetting; the dropdown; restoring the version, dimension and view; a save keyed by seed and level name.
- [x] **Viewer: sharing:** Copy view, Open view…, paste on the map, and the fallback when a version's bundle is missing.
- [x] **Viewer: import and export** of the data file, from the toolbar's menu, with a summary of what was merged and what was skipped.
- [x] **Viewer: image export** with markers, and the command with `--pin`. Extend `the_command_says_what_the_export_draws`.
- [x] **Check by eye** with `run-viewer`, from a scratch settings folder:
  - Place, name and colour markers.
  - Mark a village done.
  - Restart, and switch from 26.3 to 26.4-snapshot-6 and back.
  - Copy the view, and open it in a second viewer with another settings folder.
  - Export the data file from one settings folder and import it into another.
  - Export an image with markers.
- [x] **Docs:**
  - getting-started's viewer section: markers, done, worlds, sharing, and where the file is;
  - render.md's pins;
  - cli-json.md, if `map_features` output gains pins;
  - the changelog.
- [x] `cargo xtask check`. Complete the plan and the milestone.

## Verification

- **Survives (exit criterion 1):** a test writes markers, done places and saved worlds, reloads them in a new model, switches the world's version, and imports the exported file into an empty one. Everything comes back. `run-viewer` confirms the same in the real app.
- **View text (exit criterion 2):** round-trip tests, plus the frozen format-1 fixtures that every later change must keep opening. A fresh viewer opens a copied text to the same view, checked with `run-viewer`.
- **Hostile files (exit criterion 3):** the hand-made cases and the mutation test, with the expected problems reported for each case.
- **Exports:** pins in images have exact-pixel tests. The viewer's export and its `spyglass render --pin` command give the same bytes, as the existing export tests check for the other layers.
- **Nothing else moves:** existing render tests and goldens pass unchanged when `pins` is empty.
- `cargo xtask check` (exit criterion 4). No worldgen changes, so `oracle-test` isn't needed.

## Durable outputs

- [x] ADR: where user data lives.
- [x] `docs/specs/user-data.md`: the data file and the view text.
- [x] render.md: the pin layer.
- [x] cli-json.md: `pins` in `render`'s output.
- [x] getting-started: markers, done, saved seeds, sharing, and the file's location.

## Notes

- 2026-10-29: **Results.**
  - **Tests:** 13 for the data file (round trip, unknown fields, a newer format left alone, merging twice, deletions that stick, two writers, import and export, pruning, hand-made bad files, a file over the limit, an unreadable file moved aside, and 3,000 mutated files), 5 for the view text (with frozen format 1 texts), exact pixels for pins in the library, and `--pin` in the CLI, image bytes and JSON.
  - **By eye, with `run-viewer`,** from scratch settings folders on seed 12345:
    - A marker added with M and named. A village marked done from its menu, then dimmed and listed under Done.
    - Both survived a restart in 26.4-snapshot-6.
    - The world named and listed under Saved in Worlds.
    - The view text copied, and pasted onto the map of a fresh viewer with another settings folder, which opened the same seed, view and layers.
    - The first folder's file imported into the second through the real file dialog, which brought over the marker and the done village.
    - The export command with the marker as `--pin`, and the CLI drawing it.
  - **Not checked by eye:** opening a view text whose version has no bundle (the fallback note), and writing an image through the export dialog. The image's layers are the command's, which the CLI test checks byte for byte.
- 2026-10-29: **Changes from the approach.**
  - **Times** are integer milliseconds, not RFC 3339 strings: no date library, and they only need comparing.
  - **Done places** key a structure by its structure set and start position (`minecraft:villages`), not its structure id. A candidate's structure list changes with confirmation, and its set doesn't.
  - **Views** keep only their own fields. Unknown fields are kept at the top level and in worlds, markers and done places.
  - **Import and export** are in Settings, beside where the file is, not in the toolbar.
  - **The world name** is edited in a buffer of its own: trimming it on every keystroke ate the spaces as they were typed.
  - **No cargo-fuzz target,** as planned: the reader is in the viewer, which is a binary. The mutation test covers it.
- 2026-10-29: **Decisions confirmed in [ADR 0048](../../adr/0048-the-viewer-keeps-user-data-in-a-file-of-its-own.md).**
  - **The world key:** seed plus save name, without the version. The milestone says "a seed and version, or a save", but a key with a version would hide every marker after the user's world updates.
  - **Images:** names and done states aren't drawn, only pins.
  - **The view text:** readable `key=value` rather than base64, so it survives being typed and shows what it opens.
