# Game data

spyglass needs the game's world generation data for each game version you use. It keeps that data in a **bundle**: a folder it builds on your machine from the game's own server jar. spyglass ships nothing of Mojang's, so it downloads the jar from Mojang (about 60 MB, once per version) and builds the bundle from it. Building takes about half a minute, plus the download the first time.

You need one bundle for each game version you want to explore.

## From the viewer

Start `spyglass-viewer`. If you have no bundles yet, it offers to make one:

1. Pick where the data goes: next to spyglass, in your user folder, or somewhere else.
2. Pick a game version.
3. Press **Extract**.

To add another version later, open **Settings** in the toolbar and choose **Add a game version**.

## From a terminal

```sh
spyglass-extract 26.3
```

Run it once for each version you want. On its own, `spyglass-extract` gets the latest release, and `spyglass-extract latest-snapshot` gets the latest snapshot. To check a bundle hasn't been damaged, run `spyglass-extract verify <folder>`.

| Option              | Default                    | What it does                                          |
| ------------------- | -------------------------- | ----------------------------------------------------- |
| `--out <dir>`       | the bundles folder         | Where bundles go, each in its own `<version>` folder. |
| `--force`           | off                        | Replace an existing bundle.                           |
| `--java <path>`     | search                     | A Java home or executable to try first.               |
| `--cache-dir <dir>` | your per-user cache folder | Where downloaded server jars are kept.                |

## Java

Building a bundle runs the game's own data generator, so it needs Java, in the version the game itself needs: Java 25 for 26.3. If you've played that game version, you probably have it already, because the launcher installs it. spyglass looks for Java in:

- `JAVA_HOME`;
- your `PATH`;
- the usual install folders;
- the runtimes the official Minecraft Launcher and the Modrinth App install.

If it finds none, the error lists every place it looked. Install Java, for example [Eclipse Temurin](https://adoptium.net), or tell the extractor where it is with `--java <path>`.

Only building bundles needs Java. Nothing else in spyglass does.

## Where bundles go

Every spyglass program looks for bundles in the first of these that exists:

1. the folder named in the `SPYGLASS_BUNDLES` environment variable;
2. a `bundles` folder in the folder you run the program from;
3. a `bundles` folder next to the programs;
4. your per-user data folder:

| System  | Folder                                           |
| ------- | ------------------------------------------------ |
| Windows | `%LOCALAPPDATA%\spyglass\data\bundles`           |
| macOS   | `~/Library/Application Support/spyglass/bundles` |
| Linux   | `~/.local/share/spyglass/bundles`                |

The extractor saves bundles there too, unless you give `--out`. Every command's `--help` shows which folder it's using, and `--bundles <dir>` picks another folder for one command.

## Which game versions

```sh
spyglass --version
```

This lists the game versions this build of spyglass was checked against the real game for. A bundle for any other version still works, but spyglass warns that its answers may differ from the game's. A newer spyglass may support that version.

If you have several bundles, the `spyglass` command uses the newest release unless you pick one with `--game-version <id>`. It uses a snapshot only when you name it. With `--world`, it uses your world's own version. The viewer opens the version you used last, and you can switch with **Version** in the toolbar.
