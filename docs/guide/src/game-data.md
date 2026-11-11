# Game data

spyglass reads each game version's world generation data from a **bundle**: a folder built from the game's own server jar, on your machine. spyglass ships nothing of Mojang's. It downloads the server jar from Mojang (about 60 MB, once), checks it, and builds the bundle from it, in about 10 seconds. You need one bundle for each game version you want.

## From the viewer

Start `spyglass-viewer`. With no data yet, it offers to extract some:

1. Pick where the data goes: beside the viewer, your per-user data folder, or another folder.
2. Pick a game version.
3. Press **Extract**.

**Add a game version** in **Settings**, in the toolbar, extracts more later.

## From a terminal

```sh
spyglass-extract 26.3
```

`spyglass-extract` alone extracts the latest release, and `spyglass-extract latest-snapshot` the latest snapshot. Run it once for each version you want. `spyglass-extract verify <folder>` rechecks a bundle against the hashes it was written with.

| Option              | Default                    | What it does                                          |
| ------------------- | -------------------------- | ----------------------------------------------------- |
| `--out <dir>`       | the bundles folder         | Where bundles go, each in its own `<version>` folder. |
| `--force`           | off                        | Replace an existing bundle.                           |
| `--java <path>`     | search                     | A Java home or executable to try first.               |
| `--cache-dir <dir>` | your per-user cache folder | Where server jars are kept.                           |

## Java

Extracting runs the game's own data generator, so it needs a Java runtime of the major version the game needs: Java 25 for 26.3. If you've played that version, you probably have one already. spyglass looks in `JAVA_HOME`, on `PATH`, and in the runtimes the official Minecraft Launcher and the Modrinth App install.

Otherwise, install one, for example [Eclipse Temurin](https://adoptium.net), or point the extractor at one with `--java <path>`. If none matches, the error lists every place that was searched.

Only extracting needs Java. Once a bundle exists, nothing else does.

## Where bundles go

Every program looks for bundles in the same place:

1. the folder in `SPYGLASS_BUNDLES`, if it's set;
2. a `bundles` folder in the folder you run it from, if there is one;
3. a `bundles` folder beside the programs, if there is one;
4. otherwise, your per-user data folder:

| System  | Folder                                           |
| ------- | ------------------------------------------------ |
| Windows | `%LOCALAPPDATA%\spyglass\data\bundles`           |
| macOS   | `~/Library/Application Support/spyglass/bundles` |
| Linux   | `~/.local/share/spyglass/bundles`                |

The extractor writes to the same place unless `--out` says otherwise. Every command's `--help` shows which folder it uses, and `--bundles <dir>` overrides it for one command.

## Which game versions

```sh
spyglass --version
```

lists the game versions this build was checked against the real game for. A bundle for any other version still works, with a warning that its answers may differ from the game's. A newer spyglass may support it.

With several bundles, the commands and the viewer use the newest release unless told otherwise (`--game-version <id>`). Snapshots are used only when you name them.
