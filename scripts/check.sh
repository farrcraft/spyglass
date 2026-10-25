#!/bin/sh
# Runs the same checks CI should: formatting, lints, tests, process-doc references.
set -eu
cd "$(dirname "$0")/.."

# Roadmap and plan docs are temporary; code must not reference them.
if grep -rnE 'docs[/\\](roadmap|plans)' crates; then
    echo "error: code references docs/roadmap or docs/plans. See docs/development-process.md." >&2
    exit 1
fi

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
