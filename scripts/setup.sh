#!/bin/sh
# Installs rustup (if missing) and the toolchain pinned in rust-toolchain.toml.
set -eu
cd "$(dirname "$0")/.."

if ! command -v rustup >/dev/null 2>&1; then
    echo "rustup not found; installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
    . "$HOME/.cargo/env"
fi

# Reads rust-toolchain.toml and installs the channel and components it names.
rustup toolchain install

if ! command -v cc >/dev/null 2>&1; then
    echo "warning: no C compiler found; install build-essential (Linux) or run xcode-select --install (macOS)" >&2
fi

rustc --version
cargo --version
echo "Ready. Try: cargo build"
