# Installing

Download the archive for your system from the [releases page](https://github.com/farrcraft/spyglass/releases/latest): Windows, Linux, or macOS on Apple silicon or Intel. Unpack it into a folder of its own. No Rust and no installer are needed.

The folder holds three programs:

| Program            | What it does                                                     |
| ------------------ | ---------------------------------------------------------------- |
| `spyglass-viewer`  | The map app.                                                     |
| `spyglass`         | The command: locate, maps, searches, heights, blocks, loot.      |
| `spyglass-extract` | Builds the game data spyglass needs, once for each game version. |

It also holds `GETTING-STARTED.txt`, a short version of this guide, the changelog and the licenses.

## The first run on Windows and macOS

The programs aren't code-signed, so your system warns you the first time.

- **Windows:** if SmartScreen says "Windows protected your PC", choose **More info**, then **Run anyway**.
- **macOS:** open Terminal in the folder and run this once:

  ```sh
  xattr -dr com.apple.quarantine .
  ```

  Otherwise macOS says the programs "can't be opened". You can also allow each one in **System Settings → Privacy & Security → Open Anyway**.

## Linux

The viewer needs the usual desktop libraries. Most desktops already have them. On Debian and Ubuntu they are:

```sh
sudo apt install libxkbcommon0 libwayland-client0 libx11-6 libxcursor1 libxrandr2 libxi6 libvulkan1
```

## Checking a download

To check that an archive wasn't changed on the way, compare its SHA-256 with the one in `SHA256SUMS` on the release page, or verify its provenance with the [GitHub CLI](https://cli.github.com/):

```sh
gh attestation verify <archive> --repo farrcraft/spyglass
```

## Building from source

With [Rust](https://rustup.rs/) installed, build and install the command and the viewer from a clone of the repository:

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
