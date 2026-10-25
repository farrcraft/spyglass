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

`spyglass-viewer` will use `egui` + `wgpu`, which need windowing and graphics libraries on Linux. On Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev
```

Windows and macOS need nothing extra.

## 2. Check your machine

```sh
cargo xtask doctor
```

It checks the Rust toolchain, rustfmt and clippy, pre-commit and the git hooks, and Java (below). Each line is `ok`, `warn` or `FAIL`, with a fix for anything that isn't ok. It never installs anything, and exits non-zero if something fails. By default it checks Java against the latest release; use `--game-version <id>` for another, and `--java <path>` to check a specific installation.

### Java

The extractor runs the game's own data generator, so it needs a Java runtime matching the game version (Java 25 for 26.3). **If you've played that version, you probably have one already:** game launchers download their own runtime, and spyglass looks for it in the official Minecraft Launcher (including the Microsoft Store build) and the Modrinth App, after `JAVA_HOME` and `PATH`. Otherwise install a matching runtime, e.g. [Eclipse Temurin](https://adoptium.net) (`winget install EclipseAdoptium.Temurin.25.JRE`).

A **JDK** (a runtime plus the `javac` compiler) is only needed to work on the oracle harness. Launcher runtimes don't include `javac`. doctor warns if there's no JDK, but doesn't fail. How the oracle is built is still being decided; setup instructions will follow.

## 3. Build and test

```sh
cargo build                 # whole workspace
cargo test                  # all tests
cargo test -p spyglass-rng  # one crate
cargo run -p spyglass-viewer
cargo run -p spyglass-data --bin spyglass-extract
```

## 4. Checks and git hooks

All checks are defined in [`.pre-commit-config.yaml`](../.pre-commit-config.yaml) (see [ADR 0002](adr/0002-git-hooks-for-local-checks.md)). Once the setup script has run, they run automatically:

- **On commit**, against staged files: line endings and whitespace, JSON/YAML/TOML syntax, blocks on jars, class files and extracted game data, `cargo fmt`, TOML formatting (taplo), JSON/YAML/Markdown formatting (Prettier), Markdown lint and shellcheck. Formatters fix files in place and fail the commit; review the changes, `git add` them and commit again.
- **On push:** `cargo clippy` with warnings as errors, and `cargo test`.

To run everything against the whole repo, as CI does:

```sh
cargo xtask check
```

The first run on a machine downloads the hook environments (including Node for Prettier and markdownlint), which takes a minute or two. To update hook versions, run `pre-commit autoupdate` and review the diff.

## 5. Project commands

Project-level commands live in the `xtask` crate and run as `cargo xtask <command>` (see [ADR 0003](adr/0003-cargo-xtask-for-project-commands.md)). `cargo xtask --help` lists them. The first run compiles it, which takes a few seconds.

| Command              | What it does                                        |
| -------------------- | --------------------------------------------------- |
| `cargo xtask check`  | Every check in the pre-commit config, on all files. |
| `cargo xtask doctor` | Reports whether this machine is ready.              |

## 6. Editor setup

Use [rust-analyzer](https://rust-analyzer.github.io/). In VS Code, install the `rust-lang.rust-analyzer` extension. It picks up the workspace from the root `Cargo.toml`.

## 7. Game data (later)

Most crates will need a version bundle extracted from a Minecraft server jar. That pipeline doesn't exist yet. When it does, the steps will live here. See [specs/data-sources.md](specs/data-sources.md) for the plan. Jars and bundles stay on your machine and are gitignored.

## Next steps

- [architecture.md](architecture.md) for how the crates fit together.
- [roadmap/](roadmap/README.md) for what's being worked on.
- [development-process.md](development-process.md) for how work is planned and tracked.
