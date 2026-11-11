# Troubleshooting

## "Windows protected your PC", or "can't be opened" on macOS

The programs aren't code-signed. See [the first run](install.md#the-first-run-on-windows-and-macos).

## No Java found when extracting

Extracting needs a Java runtime of the major version the game needs: Java 25 for 26.3. The error lists every place spyglass looked. Install one, such as [Eclipse Temurin](https://adoptium.net), or point the extractor at one with `--java <path>`. See [Java](game-data.md#java).

## No bundles, or the wrong game version

Every program looks in the same [bundles folder](game-data.md#where-bundles-go), and `--help` shows which. If you extracted into another folder, set `SPYGLASS_BUNDLES` to it, or pass `--bundles`. With several versions, `--game-version <id>` picks one. Snapshots are used only when you name them.

## A warning that answers may differ from the game's

The bundle is for a game version this build of spyglass wasn't checked against. Its answers are probably right, but aren't promised to be. A newer spyglass may support it: see `spyglass --version`.

## Answers that don't match my world

See [when answers may differ](your-worlds.md#when-answers-may-differ): a preset other than Default, data packs or mods, and chunks an older version generated all change what's in a world. `spyglass world info` lists what it found.

## A structure is shown, but isn't there

It's probably a candidate: a place the game tries, where the ground decides whether it generates. Turn on **Confirm with terrain** in the viewer, or pass `--confirm` to maps and searches. See [certain and candidate](answers.md#certain-and-candidate).

## Quotes on Windows

In PowerShell and cmd, put ids and queries in double quotes: `"#minecraft:village"`. A query on several lines is easier to keep in a file, with `--query-file`.

## Features, loot or relief are slow to appear

Features and loot are drawn by placing chunks as the game does, which takes time, so they appear only when you zoom in. Relief and biomes at the surface need every column's terrain height. All of them fill in as they're worked out.

## Something else

Report it on [GitHub](https://github.com/farrcraft/spyglass/issues), with the output of `spyglass --version`, the command or view, and what you expected.
