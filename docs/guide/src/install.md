# Installing

1. Download the archive for your system from the [releases page](https://github.com/farrcraft/spyglass/releases/latest). There are archives for Windows, Linux, and macOS on Apple silicon or Intel.
2. Unpack it into a folder of its own.

That's all: there's no installer, and you don't need Rust.

The folder holds three programs:

| Program            | What it does                                                                     |
| ------------------ | -------------------------------------------------------------------------------- |
| `spyglass-viewer`  | The map app. Start here.                                                         |
| `spyglass`         | The command, for a terminal: locate, loot, maps, searches and more.              |
| `spyglass-extract` | Builds the [game data](game-data.md) spyglass needs, once for each game version. |

It also holds `GETTING-STARTED.txt` (a short version of this guide), the README, the changelog and the licenses.

## The first run on Windows and macOS

The programs aren't code-signed, so the first time you run them, your system warns you.

- **Windows:** if SmartScreen says "Windows protected your PC", choose **More info**, then **Run anyway**.
- **macOS:** open Terminal in the folder and run this once:

  ```sh
  xattr -dr com.apple.quarantine .
  ```

  Without it, macOS says the programs "can't be opened". Instead, you can allow each program in **System Settings → Privacy & Security → Open Anyway**.

## Linux

The viewer needs the usual desktop libraries, and most desktops already have them. On Debian and Ubuntu, you can install them with:

```sh
sudo apt install libxkbcommon0 libwayland-client0 libx11-6 libxcursor1 libxrandr2 libxi6 libvulkan1
```

## Checking a download

To check that an archive wasn't tampered with, compare its SHA-256 checksum with the one in `SHA256SUMS` on the release page. Or check that this repository's release workflow built it, with the [GitHub CLI](https://cli.github.com/):

```sh
gh attestation verify <archive> --repo farrcraft/spyglass
```

## Building from source

With [Rust](https://rustup.rs/) installed, build and install all three programs from a copy of the repository:

```sh
git clone https://github.com/farrcraft/spyglass
cd spyglass
cargo install --path crates/spyglass-cli
cargo install --path crates/spyglass-viewer
cargo install --path crates/spyglass-extract
```

The [contributor's getting started](https://github.com/farrcraft/spyglass/blob/main/docs/getting-started.md) covers working on spyglass itself.

## Next

[Get the game data](game-data.md).
