# Getting started

## 1. Install Rust

spyglass uses the stable Rust toolchain through [rustup](https://rustup.rs). The repo's `rust-toolchain.toml` pins the channel and components, so once rustup is installed, `cargo` picks the right toolchain automatically.

### Option A: setup script

The setup scripts install rustup if it is missing, then install the pinned toolchain plus `rustfmt` and `clippy`, and print the versions.

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

**Windows**

1. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the "Desktop development with C++" workload. Rust's default Windows target uses the MSVC linker.
2. Install rustup, either with `winget install Rustlang.Rustup` or by downloading `rustup-init.exe` from [rustup.rs](https://rustup.rs).
3. Open a new terminal so `cargo` is on your `PATH`.

**macOS / Linux**

1. Make sure a C toolchain is available (`xcode-select --install` on macOS; `build-essential` or your distro's equivalent on Linux).
2. Run:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. Restart your shell, or run `. "$HOME/.cargo/env"`.

**Check it worked**

```sh
rustc --version
cargo --version
```

Run these from inside the repo so rustup applies `rust-toolchain.toml`.

### Linux viewer dependencies

`spyglass-viewer` will use `egui` + `wgpu`, which need windowing and graphics libraries on Linux. On Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev
```

Windows and macOS need nothing extra.

## 2. Build and test

```sh
cargo build                 # whole workspace
cargo test                  # all tests
cargo test -p spyglass-rng  # one crate
cargo run -p spyglass-viewer
cargo run -p spyglass-data --bin spyglass-extract
```

Before pushing, run the same checks CI should:

```powershell
./scripts/check.ps1
```

```sh
./scripts/check.sh
```

These run `cargo fmt --check`, `cargo clippy` with warnings as errors, and `cargo test`.

## 3. Editor setup

Use [rust-analyzer](https://rust-analyzer.github.io/). In VS Code, install the `rust-lang.rust-analyzer` extension. It picks up the workspace from the root `Cargo.toml`.

## 4. Game data (later)

Most crates will need a version bundle extracted from a Minecraft server jar. That pipeline doesn't exist yet. When it does, the steps will live here. See [specs/data-sources.md](specs/data-sources.md) for the plan. Jars and bundles stay on your machine and are gitignored.

## Next steps

- [architecture.md](architecture.md) for how the crates fit together.
- [roadmap.md](roadmap.md) for what's being worked on.
