# Getting started

## 1. Install Rust and tools

spyglass uses the stable Rust toolchain through [rustup](https://rustup.rs). The repo's `rust-toolchain.toml` pins the channel and components, so once rustup is installed, `cargo` picks the right toolchain automatically.

### Option A: setup script

The setup scripts:

1. install rustup if it is missing, then the pinned toolchain plus `rustfmt` and `clippy`;
2. install [uv](https://github.com/astral-sh/uv) if it is missing, then [pre-commit](https://pre-commit.com) through uv (uv brings its own Python, so you don't need one);
3. enable the repo's git hooks with `pre-commit install`;
4. run `cargo xtask doctor` to report anything still missing (see [step 2](#2-check-your-machine)).

They're safe to re-run.

```powershell
# Windows (PowerShell)
./scripts/setup.ps1
```

```sh
# macOS / Linux
./scripts/setup.sh
```

If PowerShell refuses to run the script, allow local scripts for your user once:

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

### Option B: by hand

#### Windows

1. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload. Rust's default Windows target uses the MSVC linker.
2. Install rustup, either with `winget install Rustlang.Rustup` or by downloading `rustup-init.exe` from [rustup.rs](https://rustup.rs).
3. Open a new terminal so `cargo` is on your `PATH`.

#### macOS / Linux

1. Make sure a C toolchain is available (`xcode-select --install` on macOS; `build-essential` or your distro's equivalent on Linux).
2. Run:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. Restart your shell, or run `. "$HOME/.cargo/env"`.

#### Then, on any platform

Install pre-commit (or [prek](https://github.com/j178/prek), which reads the same config and needs no Python), and enable the hooks from inside the repo:

```sh
uv tool install pre-commit   # or: prek
pre-commit install           # or: prek install
```

### Linux viewer dependencies

`spyglass-viewer` uses `egui` + `wgpu` ([ADR 0019](adr/0019-gui-stack-and-msrv-1-95.md)), which need windowing and graphics libraries on Linux. On Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev
```

Windows and macOS need nothing extra.

## 2. Check your machine

```sh
cargo xtask doctor
```

It checks the Rust toolchain, rustfmt and clippy, pre-commit and the git hooks, Java (below), and Docker (only a warning; just the [Linux check](#linux-check) and the [dev container](#dev-container) need it). Each line is `ok`, `warn` or `FAIL`, with a fix for anything that isn't ok. It never installs anything, and exits non-zero if something fails. By default it checks Java against the latest release; use `--game-version <id>` for another, and `--java <path>` to check a specific installation.

### Java

The extractor runs the game's own data generator, so it needs a Java runtime matching the game version (Java 25 for 26.3). **If you've played that version, you probably have one already:** game launchers download their own runtime, and spyglass looks for it in the official Minecraft Launcher (including the Microsoft Store build) and the Modrinth App, after `JAVA_HOME` and `PATH`. Otherwise install a matching runtime, e.g. [Eclipse Temurin](https://adoptium.net) (`winget install EclipseAdoptium.Temurin.25.JRE`).

A **JDK** (a runtime plus the `javac` compiler) is only needed to work on the oracle harness, which is compiled against the game's server jar ([ADR 0004](adr/0004-oracle-standalone-harness.md)). Launcher runtimes don't include `javac`. doctor warns if there's no JDK, but doesn't fail. The JDK's major version must match the game's (25 for 26.3). To install one:

```powershell
./scripts/setup.ps1 -Oracle   # Windows: Eclipse Temurin via winget
```

```sh
./scripts/setup.sh --oracle   # macOS: Temurin via Homebrew; Linux: prints what to install
```

Both skip the install if a matching JDK is already there. On Linux, install your distro's `openjdk-25-jdk` (or equivalent), use [SDKMAN](https://sdkman.io), or get [Temurin](https://adoptium.net). spyglass finds JDKs in the usual install folders even when they aren't on `PATH`. No Gradle or other Java build tool is needed: the oracle is built with plain `javac`.

## 3. Build and test

```sh
cargo build                 # whole workspace
cargo test                  # all tests (or: cargo nextest run, as the hooks do)
cargo test -p spyglass-rng  # one crate
cargo run -p spyglass-extract
```

## 4. Checks and git hooks

All checks are defined in [`.pre-commit-config.yaml`](../.pre-commit-config.yaml) (see [ADR 0002](adr/0002-git-hooks-for-local-checks.md) and [ADR 0005](adr/0005-extra-rust-checks.md)). Once the setup script has run, they run automatically:

- **On commit**, against staged files: line endings and whitespace, JSON/YAML/TOML syntax, blocks on jars, class files and extracted game data, relative Markdown links and anchors (`cargo xtask links`), `cargo fmt`, unused dependencies (cargo-shear), spelling (typos), TOML formatting (taplo), JSON/YAML/Markdown formatting (Prettier), Markdown lint and shellcheck. Formatters fix files in place and fail the commit; review the changes, `git add` them and commit again.
- **On push:** `cargo clippy` with warnings as errors, tests (`cargo nextest run`, then `cargo test --doc` for doctests), `cargo doc` with the rustdoc lints, a build with the minimum supported Rust version (`cargo xtask msrv`), and `cargo-deny` (security advisories, licenses, dependency sources; configured in [`deny.toml`](../deny.toml)).

Lint levels for every crate are set once, in `[workspace.lints]` in the root `Cargo.toml`. A new crate opts in with `[lints] workspace = true`.

The setup scripts install the extra tools the hooks need: `cargo-nextest`, `cargo-shear` and the MSRV toolchain. To install them by hand:

```sh
cargo install --locked cargo-nextest@0.9.149 cargo-shear@1.14.0
rustup toolchain install "$(cargo xtask msrv --print)" --profile minimal
```

To run everything against the whole repo, as CI does:

```sh
cargo xtask check
```

The first run on a machine downloads the hook environments (including Node for Prettier and markdownlint), which takes a minute or two. To update hook versions, run `pre-commit autoupdate` and review the diff.

### Linux check

With Docker installed, check the committed `HEAD` on a clean Ubuntu 24.04 machine ([ADR 0007](adr/0007-dev-container-and-linux-check.md)):

```sh
cargo xtask linux-check          # fresh clone, scripts/setup.sh, Java, doctor, extract, verify
cargo xtask linux-check --full   # also cargo xtask check: every hook, on Linux
```

It builds the `base` stage of `.devcontainer/Dockerfile` (only git, curl and a C toolchain), clones `HEAD` into a container and checks each step: the setup script, the default toolchain, the extractor's error with no Java, then with a Java runtime installed, doctor, extraction and a clean `git status`. Uncommitted changes aren't included. A run takes about six minutes, most of it compiling the cargo tools and installing hook environments; `--full` adds only a minute or two. `--keep` leaves the container running to look around in.

## 5. Project commands

Project-level commands live in the `xtask` crate and run as `cargo xtask <command>` (see [ADR 0003](adr/0003-cargo-xtask-for-project-commands.md)). `cargo xtask --help` lists them. The first run compiles it, which takes a few seconds.

| Command                    | What it does                                                                        |
| -------------------------- | ----------------------------------------------------------------------------------- |
| `cargo xtask bench`        | Times the public API's main operations (`dist` build).                              |
| `cargo xtask check`        | Every check in the pre-commit config, on all files.                                 |
| `cargo xtask ci`           | One game version's CI job, from extraction to comparison.                           |
| `cargo xtask doctor`       | Reports whether this machine is ready.                                              |
| `cargo xtask links`        | Checks every relative link and anchor in the Markdown.                              |
| `cargo xtask linux-check`  | Checks a fresh clone on a clean Linux machine, in Docker.                           |
| `cargo xtask msrv`         | Builds the workspace with its minimum Rust version.                                 |
| `cargo xtask oracle`       | Builds and runs the oracle harness (below).                                         |
| `cargo xtask oracle-test`  | Runs the ignored comparisons with the game (below).                                 |
| `cargo xtask version-diff` | Reports what changed between two bundles ([adding a version](adding-a-version.md)). |

## 6. The oracle

The oracle asks the real game for answers and writes golden files that the Rust crates are tested against ([specs/oracle.md](specs/oracle.md)). It needs a JDK (see [Java](#java)). Everything it downloads or generates stays out of the repo: the server jar goes to your per-user cache, build output to `target/oracle/`, and golden files to `oracle-out/` (gitignored).

```sh
cargo xtask oracle golden                        # regenerate all golden files for the latest release
cargo xtask oracle golden --game-version 26.3    # or for a specific version
cargo xtask oracle check-save "<path to a world>" # check the harness against a save made in that version
```

The first run downloads the server jar (about 60 MB) and compiles the harness. Startup takes about 10 seconds; the full golden set about a minute more on a few cores. Golden-file tests are `#[ignore]`d, since the files are local. `cargo xtask oracle-test` runs every one whose files and bundle exist, in a release build, once per game version in `bundles/` (or `--game-version 26.3` for one), and says which it skipped and how each version went; or run a crate's with `cargo test -- --ignored`, as below, which checks the newest release in `bundles/`, or the version in `SPYGLASS_ORACLE_VERSION`.

`cargo xtask oracle probe` (also part of `golden`) writes the game's raw random-source, noise and density-function values into `oracle-out/<version>/probe/`. The RNG, noise and density crates check against them, and the noise and density tests also need a bundle for the same version ([Game data](#8-game-data)):

```sh
cargo test --release -p spyglass-rng -p spyglass-noise -p spyglass-density -- --ignored
```

The biome crate checks every cell of the golden biome files, and needs the golden set and a bundle for the same version. It takes a few seconds per million cells per core (about 15 seconds for 26.3 on a 16-core machine):

```sh
cargo test --release -p spyglass-biome -- --ignored
```

`cargo xtask oracle structures` (also part of `golden`) writes the structure golden files: placement chunks, what the game tries in each, ring positions and slime chunks for each golden seed. The structure crate checks against them in about 40 seconds. It needs a bundle with structure templates, so extract again if yours predates them:

```sh
cargo test --release -p spyglass-structure -- --ignored
```

`cargo xtask oracle spawn` (also part of `golden`) writes the spawn golden files: the chunk the game's spawn search starts from, and the climate search behind it. The biome crate's ignored tests above include their comparison, which takes under a second on its own:

```sh
cargo test --release -p spyglass-biome --test spawn -- --ignored
```

The other subcommands each do one part of the work, and `cargo xtask oracle --help` describes them: `build` compiles the harness (the others do it first when anything is stale), `biomes` writes one golden biome file for a seed and region, `heights` writes the height golden files (part of `golden`), `locate` asks a real dedicated server `/locate` questions and takes several minutes (not part of `golden`), and `test-pack` checks the forms vanilla data doesn't use ([specs/oracle.md](specs/oracle.md)).

`check-save` takes any world created in the same game version with the Default preset and no worldgen datapacks. Its folder is in your launcher's `saves` directory. It should report zero unexplained mismatches.

## 7. Editor setup

Use [rust-analyzer](https://rust-analyzer.github.io/). In VS Code, install the `rust-lang.rust-analyzer` extension. It picks up the workspace from the root `Cargo.toml`.

### Dev container

For a ready-made Linux environment, open the repo in VS Code with the Dev Containers extension and Docker installed, then run **Dev Containers: Reopen in Container**. The container (`.devcontainer/`, [ADR 0007](adr/0007-dev-container-and-linux-check.md)) is Ubuntu 24.04 with a JDK 25 and the viewer's build libraries. On creation it runs `scripts/setup.sh`, which installs Rust, the cargo tools, pre-commit and the git hooks. The first creation takes about five minutes.

- `~/.cargo`, `~/.rustup` and the build output (`CARGO_TARGET_DIR=~/target`) live in named Docker volumes, so rebuilding the container is quick and Linux builds don't mix with the host's `target/`.
- The git hooks are shared with the host checkout. They work on both sides, because pre-commit's hook script falls back to whichever `pre-commit` is on `PATH`.
- The viewer builds in the container but needs a display to run, so run it on the host.
- When the target game version needs a new Java major, update the JDK package in the Dockerfile.

## 8. Game data

Most crates need a **version bundle**: the game's worldgen data for one version, extracted from the server jar on your machine ([specs/bundle-format.md](specs/bundle-format.md)). It needs a Java runtime matching that version (see [Java](#java)).

```sh
cargo run -p spyglass-extract -- 26.3     # or: latest, latest-snapshot
cargo run -p spyglass-extract -- verify bundles/26.3
```

The extractor downloads the server jar (about 60 MB, once, into your per-user cache) and checks its SHA-1. It copies the worldgen JSON out of the jar and runs the game's data generator for the biome parameter lists that only exist in code, then writes `bundles/<version>/` with a `bundle.json` listing every file's hash. That takes about 10 seconds. `verify` rechecks a bundle against its `bundle.json`.

| Option              | Default                    | What it does                                          |
| ------------------- | -------------------------- | ----------------------------------------------------- |
| `--out <dir>`       | `bundles`                  | Where bundles go; each in its own `<version>` folder. |
| `--force`           | off                        | Replace an existing bundle.                           |
| `--java <path>`     | search                     | A Java home or executable to try first.               |
| `--cache-dir <dir>` | your per-user cache folder | Where server jars are kept.                           |

If no matching Java is found, the error lists every place that was searched. Install a runtime of the right major version, or point `--java` at one. Jars and bundles stay on your machine: the cache is outside the repo, and `bundles/` is gitignored.

## 9. The `spyglass` command

With a bundle in `bundles/`, the `spyglass` command answers `/locate`-style questions for any seed, the way the game's `/locate` would ([specs/locate.md](specs/locate.md)):

```sh
cargo run --release -p spyglass-cli -- locate structure '#minecraft:village' --seed 12345
cargo run --release -p spyglass-cli -- locate structure minecraft:stronghold --seed spyglass --from 1000,64,-2000
cargo run --release -p spyglass-cli -- locate biome minecraft:cherry_grove --seed 12345
cargo run --release -p spyglass-cli -- locate slime --seed 12345 --radius 4
cargo run --release -p spyglass-cli -- locate spawn --seed 12345
cargo run --release -p spyglass-cli -- locate structure minecraft:bastion_remnant --seed 12345 --dimension nether
cargo run --release -p spyglass-cli -- heights --seed 12345 --min 0,0 --max 15,15
```

Or install it with `cargo install --path crates/spyglass-cli` and run `spyglass ...` directly.

| Option                | Default        | What it does                                                                              |
| --------------------- | -------------- | ----------------------------------------------------------------------------------------- |
| `--seed <seed>`       | required       | A number, or text hashed as the world-creation screen does.                               |
| `--world <save>`      | none           | A world save instead of `--seed`: its folder, or its folder name in a launcher's saves.   |
| `--from <x,y,z>`      | `0,64,0`       | Where to search from, in blocks; `x,z` means y 64; `player` is `--world`'s single player. |
| `--dimension <d>`     | Overworld      | `overworld`, `nether` or `end`. With `--from player`, the player's dimension by default.  |
| `--radius <n>`        | as `/locate`   | Placement-cell rings (structures), blocks (biomes) or chunks (slime).                     |
| `--unconfirmed`       | off            | Structures: don't decide candidates with the seed's terrain. Faster, but less certain.    |
| `--bounds`            | off            | Structures: each certain jigsaw structure's bounds and piece count.                       |
| `--game-version <id>` | newest release | Which bundle in `--bundles` to use. Snapshots are used only when named.                   |
| `--bundle <dir>`      | none           | Use this bundle directory instead.                                                        |
| `--bundles <dir>`     | `bundles`      | Where bundles are.                                                                        |
| `--json`              | off            | Print JSON for scripts ([specs/cli-json.md](specs/cli-json.md)).                          |

Slime chunks and the world spawn are only in the Overworld. In the Nether, heights find the solid ceiling (about 128 everywhere), which is what the game's own structure checks see too.

`spyglass heights` prints the generator's terrain heights for one column (`--at x,z`) or a region (`--min x,z --max x,z`, up to 256 × 256 columns): one above the highest block before structures, carvers, features and surface blocks change it, as structures are placed by ([specs/terrain.md](specs/terrain.md)). `--heightmap ocean-floor` ignores water.

### Your own worlds

```sh
cargo run --release -p spyglass-cli -- world list                     # saves in the official launcher and Modrinth App
cargo run --release -p spyglass-cli -- world info "My World"          # seed, version, spawn, player, warnings
cargo run --release -p spyglass-cli -- locate structure '#minecraft:village' --world "My World" --from player
```

`world info` says whether spyglass can answer for a world, and what makes its answers inexact: no bundle for the world's version, a preset other than Default, data packs or mods that may change generation, and chunks an older version generated ([specs/world-saves.md](specs/world-saves.md)). It reads every region file to count those chunks, which takes a few seconds for a large world; `--quick` skips that. spyglass never writes to a save.

### Maps

`spyglass render` draws a map as a PNG: biomes, and the layers you ask for ([specs/render.md](specs/render.md)). It's the same image the viewer exports for the same view.

```sh
cargo run --release -p spyglass-cli -- render --seed 12345 -o map.png
cargo run --release -p spyglass-cli -- render --seed 12345 --centre 1000,-2000 --size 1920x1080 --scale 16 \
    --structures '#minecraft:village,minecraft:stronghold' --slime --spawn -o map.png
cargo run --release -p spyglass-cli -- render --world "My World" --highlight minecraft:cherry_grove -o map.png
```

| Option                            | Default            | What it does                                                                             |
| --------------------------------- | ------------------ | ---------------------------------------------------------------------------------------- |
| `-o <file>`                       | required           | The PNG to write.                                                                        |
| `--centre <x,z>` or `--min <x,z>` | 0,0, or the player | The block at the image's centre, or at its top-left corner.                              |
| `--size <WxH>`                    | `1024x1024`        | Pixels.                                                                                  |
| `--scale <n>`                     | `4`                | Blocks per pixel: 1, 2, 4 and so on up to 256.                                           |
| `--y <y>`                         | `64`               | The height biomes are drawn at. The map is a slice at this height, not the surface.      |
| `--surface`                       | off                | Biomes at the surface instead of at `--y` (scales up to 4).                              |
| `--relief`                        | off                | Shade by terrain height (scales up to 4). Slow: about a second per 256 × 256 at scale 1. |
| `--structures <ids>`              | none               | Structure ids or `#tags` to mark, separated by commas. Hollow markers are candidates.    |
| `--confirm`                       | off                | Decide structure markers with the seed's terrain, so nearly all are certain.             |
| `--highlight <ids>`               | none               | Biomes to keep bright; the rest are dimmed.                                              |
| `--slime`, `--spawn`, `--ties`    | off                | Slime chunks (scales up to 16), the spawn's origin, tied biome cells.                    |
| `--no-biomes`                     | off                | A plain background under the other layers, or grey by height with `--relief`.            |

The seed, world, dimension and bundle options are as for `locate`; with `--world`, a map centred on the player is in the player's dimension. In the Nether, terrain layers aren't drawn, since its surface heights are the ceiling's, and slime chunks and the spawn are the Overworld's alone. With `--json` it prints the area and everything it drew.

### Seed search

`spyglass search` finds the seeds that best fit a query ([specs/search.md](specs/search.md)). Each line of a query is a constraint: `require` ones a seed must meet, `prefer` ones that add to its score, with `weight N` to count one more.

```sh
cargo run --release -p spyglass-cli -- search --time 2m -q 'require 2 minecraft:swamp_hut within 600 of spawn
require cluster 2 minecraft:swamp_hut within 200 of each other within 600 of spawn
prefer 3 #minecraft:village within 800 of spawn
prefer biome minecraft:cherry_grove within 500 of spawn weight 2'
cargo run --release -p spyglass-cli -- search --query-file huts.txt --range -2147483648..2147483647 --count 1000000
cargo run --release -p spyglass-cli -- search --time 2m -q 'require minecraft:fortress within 100 of spawn
prefer minecraft:bastion_remnant within 200 of minecraft:fortress within 1000 of spawn'
```

Each constraint is in one dimension, inferred from what it counts: fortresses and bastions are the Nether's, end cities the End's, villages the Overworld's. `spawn` in a Nether constraint is the block the Overworld spawn's portal leads to (its position divided by 8), and a structure anchor in another dimension is converted the same way. Write `nether`, `end` or `overworld` after `require` or `prefer` to choose for a structure tag that spans dimensions: `require nether #minecraft:ruined_portal within 300 of spawn` ([specs/search.md](specs/search.md#dimensions)).

| Option                          | Default    | What it does                                                                                       |
| ------------------------------- | ---------- | -------------------------------------------------------------------------------------------------- |
| `-q <text>` or `--query-file`   | required   | The query.                                                                                         |
| `--count <n>`, `--time <t>`     | none       | Stop after this many seeds, or this long (`90s`, `10m`, `2h`). Without either, Ctrl-C stops it.    |
| `--range <a..b>`, `--seeds <f>` | every seed | Only seeds from a to b (both included; the example is every text seed), or those listed in a file. |
| `--from <index>`                | `0`        | Where to start; a search prints the index to continue from.                                        |
| `--top <n>`                     | `10`       | How many seeds to keep.                                                                            |
| `--confirm`                     | off        | Decide structure starts with each seed's terrain, as the game does.                                |
| `--certain`                     | off        | Only seeds whose required constraints hold whatever is left undecided.                             |
| `--threads <n>`                 | all cores  | Worker threads.                                                                                    |

The bundle options are as for `locate`; `--json` prints every hit with what each constraint matched. Searching every seed checks structure placement first, which depends only on the seed's lower 48 bits, so seeds that can't place enough structures are skipped 65,536 at a time. The same query, range, start and count always give the same list, on any machine.

Most structures need terrain to confirm. Without `--confirm`, a constraint on them is usually **possibly** met rather than met: the structures are where the game tries them, and whether each one generates depends on the ground there. With it, terrain decides them, and only tied biomes and platform-dependent results stay uncertain. Confirming costs little on top of a search, since it runs only on seeds that pass everything else.

### Reading the answers

Answers are listed in the game's search order, which isn't always nearest first. `locate` decides each structure with the seed's terrain, as the game does, so its answers are almost always **certain**, and then exactly the game's `/locate` answer. A **candidate** is left where the game's choice depends on a tied biome cell, a platform-dependent stronghold ring position or a mineshaft spyglass can't place yet: the game's answer is the first of them it generates, usually the first one listed. Without terrain (`--unconfirmed`, and maps and searches unless they confirm), most surface structures (villages, temples, monuments) are candidates.

## 10. The viewer

```sh
cargo run --release -p spyglass-viewer
```

The viewer opens the newest release in `bundles/` (or the newest snapshot if there's no release) (choose another folder from the side panel) and shows seed 0. Without a bundle it says how to extract one.

- **Move around:** drag to pan, scroll to zoom around the pointer, or use the arrow keys and `+`/`-`. **Go to** takes `x z` or `x y z`. The status bar shows the block, chunk and biome under the pointer.
- **Seed or save:** type a seed and press **Show**, or open a save from the launchers' list or any folder. A save opens on its player, in the player's dimension, with anything that makes its answers inexact listed.
- **Dimension:** switch between the Overworld, the Nether and the End; each keeps its own view. Slime chunks and the spawn are the Overworld's, and terrain layers aren't drawn in the Nether, whose surface is its ceiling.
- **Layers:** tied biome cells, slime chunks, the world spawn, structures to mark (hover a marker for what it is), and biomes to highlight. **Height** sets the y biomes are drawn at. Up close (4 blocks per pixel or less), **Relief** shades the map by terrain height and **Biomes at the surface** draws each column's biome at its height; both are slow to fill in. **Confirm with terrain** (on by default) decides structure markers and locates with the seed's terrain, and draws the bounds of villages and other jigsaw structures up close. The status bar shows the terrain height under the pointer.
- **Locate:** right-click the map for the nearest structure or biome from there, as `/locate` would. Hits are listed in the side panel (click one to go there) and ringed on the map.
- **Export:** **Export image…** saves the view as a PNG, and **Copy command** copies the `spyglass render` command that draws the same image.
- **Search:** type a query in the **Search** section, set a seed count or time limit (and **Confirm with terrain** to decide structures), and press **Search**. The best seeds appear as they're found; click one to open it, in its first constraint's dimension and centred on that constraint's place, with everything its constraints matched there ringed.

Rendering runs in the background on all but one core, so the map stays responsive while tiles fill in. Settings are kept between runs.

## Next steps

- [architecture.md](architecture.md) for how the crates fit together.
- [roadmap/](roadmap/README.md) for what's being worked on.
- [development-process.md](development-process.md) for how work is planned and tracked.
