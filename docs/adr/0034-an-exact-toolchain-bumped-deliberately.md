# 0034. An exact toolchain, bumped deliberately

- Status: Accepted
- Date: 2026-10-28

## Context

`rust-toolchain.toml` named the channel `stable`, so each machine and each CI run used whatever stable rustup had at the time. The hooks run `cargo clippy -- -D warnings` ([ADR 0005](0005-extra-rust-checks.md)), and every Rust release adds and tightens lints. A new release could therefore turn CI red, or make a commit fail its hooks on one machine and pass on another, with no change to the code. The breakage arrives at a time nobody chose, mixed into unrelated work.

The minimum supported Rust version ([ADR 0019](0019-gui-stack-and-msrv-1-95.md), `rust-version` in `Cargo.toml`) is a separate question. It says how old a compiler the code must build with, and `cargo xtask msrv` checks it. It doesn't say which compiler development and CI use.

## Decision

We will pin `rust-toolchain.toml` to an exact release (1.99.1 today) and bump it deliberately, in a commit of its own that fixes whatever the new release's clippy finds. `cargo xtask doctor` reports the pin, and warns when an installed `stable` toolchain is newer.

## Consequences

- Every machine and CI run builds and lints with the same compiler. A failed check means the code changed, not the toolchain.
- A Rust upgrade is a reviewable change, with its lint fixes in the same commit.
- Someone has to bump it. Dependabot doesn't update `rust-toolchain.toml`. `doctor` only knows about releases rustup has already installed, so it's a reminder, not a feed. The pin can drift behind if nobody runs `rustup update`.
- Each bump downloads a toolchain on every machine; rustup does that on the first `cargo` call.
- The MSRV toolchain stays separate and unaffected.

## Alternatives considered

- **Keep `stable`:** always current, but upgrades land unplanned and machines can disagree.
- **Pin a nightly date:** reproducible too, but the project needs no nightly features, and nightlies break more often. Fuzzing uses nightly on its own, outside this pin ([ADR 0036](0036-fuzzing-with-cargo-fuzz.md)).
- **Pin to the MSRV:** one toolchain fewer, but it would hold development back to the oldest supported compiler and its older lints.
