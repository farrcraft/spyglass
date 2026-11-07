---
name: run-viewer
description: Launch and drive the spyglass viewer on Windows to see a change working, by eye. Start it from known settings (seed, version, dimension, view, layers, structures) without touching the user's own, click, right-click, hover, scroll and screenshot it. Use when asked to run, check or screenshot the viewer, or to confirm a viewer change works in the real app rather than only in tests.
---

# Running the viewer

`viewer.ps1` beside this file drives `target/dist/spyglass-viewer.exe` through Windows input and screen capture. Run it with the PowerShell tool (Windows PowerShell 5.1). It's for checks by eye. Timings belong to `cargo xtask viewer-timing`, and correctness belongs to tests.

## Steps

1. **Build** the viewer in the profile it's run from. Stop any running viewer first: on Windows a running exe can't be overwritten, and cargo fails with "Access is denied".

   ```powershell
   & .claude/skills/run-viewer/viewer.ps1 stop
   $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"; cargo build --profile dist -p spyglass-viewer
   ```

2. **Start** it from settings written for the check, into a folder in the scratchpad. `SPYGLASS_VIEWER_SETTINGS` points the viewer at that file, so the user's own settings (`%APPDATA%\spyglass\data\app.ron`) are left alone. The window opens at 1400 × 900 at the screen's top-left.

   ```powershell
   $V = ".claude/skills/run-viewer/viewer.ps1"
   & $V start -Out <scratchpad>\viewer -Seed 12345 -Version 26.3 -Structures minecraft:ancient_city,minecraft:igloo
   ```

   Options: `-Dimension overworld|the_nether|the_end`, `-CentreX`/`-CentreZ` (blocks), `-Scale` (blocks per pixel), `-Surface`, `-Relief`, `-Unconfirmed`, and `-Wait` (seconds before returning, default 6). Pass structures as ids. Tags work too, and the viewer turns them into their members. `-Features` takes placed feature ids (`minecraft:monster_room,minecraft:monster_room_deep`), drawn at scales up to 2. `-Containers` marks every loot container and `-Loot` rings those holding an item (`minecraft:diamond`), both at scales up to 2; click a container for its contents.

3. **Look** at it: `& $V shot -Out <scratchpad>\viewer\name.png`, then Read the PNG. A blank or grey frame means it didn't draw: wait longer, or check that a bundle for `-Version` is in `bundles/`.

4. **Interact**, then take another shot:

   - `click -X -Y`: a left click.
   - `rclick -X -Y`: the map's menu, with Nearest structure and Nearest biome.
   - `move -X -Y`: hover, which opens a submenu.
   - `wheel -X -Y -Notches n`: scroll a list, or zoom the map. Positive scrolls down.
   - `key -Keys k`: type keys in `SendKeys` notation (`n`, `{ESC}`). The map's keys work only while no text field has focus, so click the map first.

   Coordinates are screen pixels. `shot` prints where the window is (`window at 7,0`), so a point at (x, y) in the screenshot is at (x + left, y + top) on screen. The few pixels of difference rarely matter for buttons.

5. **Stop** it when done: `& $V stop`.

## Layout at 1400 × 900

These are positions in the screenshot, as of the redesigned viewer. Take a shot before relying on them.

- **Tabs:** Layers, Search and Locate are at the top of the side panel, around y 110.
- **Structure list:** the checkboxes are at x 25, starting at y 518 and 30 px apart, in alphabetical order by family.
- **The map:** x 302 to 1400, y 88 to 898. Its centre, about (851, 493), is the view's centre block.
- **The right-click menu:** opens at the pointer. "Nearest structure" is its first item. The submenu lists every structure alphabetically, starting with 18 abandoned-camp kinds, so scroll it with `wheel` over the submenu.
- **Status bar:** the bottom row. It shows the block under the pointer and "Drawing, n left" while tiles are still coming.

## Notes

- Run one viewer at a time: `start` stops any running one, and `shot` captures the first it finds.
- The screen must be unlocked, and nothing may cover the window: capture copies the screen, and input goes to whatever is under the pointer.
- If the viewer opens without game data, its first-run panel asks to extract some. Point `-Version` at a bundle that's already in `bundles/`.
- Report what the screenshots showed, and say when a check was by eye only.
