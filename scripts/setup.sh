#!/bin/sh
# Installs rustup (if missing) and the toolchain pinned in rust-toolchain.toml,
# then uv, pre-commit and the git hooks.
set -eu
cd "$(dirname "$0")/.."

if ! command -v rustup >/dev/null 2>&1; then
    echo "rustup not found; installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
fi

# Reads rust-toolchain.toml and installs the channel and components it names.
rustup toolchain install

if ! command -v cc >/dev/null 2>&1; then
    echo "warning: no C compiler found; install build-essential (Linux) or run xcode-select --install (macOS)" >&2
fi

# uv provides pre-commit (and its own Python). See docs/adr/0002-git-hooks-for-local-checks.md.
if ! command -v uv >/dev/null 2>&1; then
    echo "uv not found; installing..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
fi
PATH="$HOME/.local/bin:$PATH"
export PATH

uv tool install pre-commit

# Installs the hook types listed in default_install_hook_types.
pre-commit install

# Reports anything still missing. See docs/adr/0003-cargo-xtask-for-project-commands.md.
cargo xtask doctor
