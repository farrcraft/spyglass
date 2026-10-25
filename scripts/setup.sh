#!/bin/sh
# Installs rustup (if missing) and the toolchain pinned in rust-toolchain.toml,
# then uv, pre-commit and the git hooks. --oracle also installs the JDK that
# oracle work needs.
set -eu
cd "$(dirname "$0")/.."

oracle=0
for arg in "$@"; do
    case "$arg" in
        --oracle) oracle=1 ;;
        *) echo "usage: $0 [--oracle]" >&2; exit 2 ;;
    esac
done

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

# Oracle work compiles a Java harness, so it needs a JDK, not just a runtime.
# See docs/adr/0004-oracle-standalone-harness.md.
if [ "$oracle" = 1 ]; then
    status=$(cargo xtask jdk-status)
    major=${status% *}
    if [ "${status#* }" = found ]; then
        echo "JDK $major already installed."
    elif [ "$(uname)" = Darwin ] && command -v brew >/dev/null 2>&1; then
        brew install --cask "temurin@$major"
    else
        echo "Install a JDK $major: your distro's openjdk-$major-jdk package, SDKMAN (sdk list java)," >&2
        echo "or Eclipse Temurin from https://adoptium.net. Then re-run this script." >&2
    fi
fi

# Reports anything still missing. See docs/adr/0003-cargo-xtask-for-project-commands.md.
cargo xtask doctor
