# 0009. MD5 from the md-5 crate

- Status: Accepted
- Date: 2026-10-25

## Context

The game seeds every noise from its name: `RandomSupport.seedFromHashOf(String)` takes the MD5 of the name's UTF-8 bytes, and `XoroshiroPositionalRandomFactory.fromHashOf` builds a random source from it. Each noise in a seed's `RandomState` is created this way, so `spyglass-rng`, the lowest crate in the stack, needs MD5.

`spyglass-rng` has no dependencies yet. Everything above it depends on it, so whatever it pulls in, every crate pulls in.

## Decision

We will use RustCrypto's **`md-5`** crate (`0.11`), added through `[workspace.dependencies]`, as `spyglass-rng`'s only dependency. It's used only for name seeding, and the port of `seedFromHashOf` is tested against RFC 1321's test suite as well as against the oracle's probe values.

## Consequences

- No hand-written cryptographic code to maintain. MD5 is used here for seeding, not security, but a correct, widely used implementation removes one place for a parity bug.
- It's the same family as the `sha1` crate that `spyglass-data` already uses, and shares its `digest` 0.11 base, so the dependency tree barely grows. Its license (MIT or Apache-2.0) is already allowed in `deny.toml`, and its MSRV (1.85) is below the workspace's.
- `spyglass-rng` is no longer dependency-free. Any future dependency there should meet the same bar: small, widely used, and hard to get exactly right by hand.

## Alternatives considered

- **Hand-written MD5** (about 100 lines, tested against RFC 1321): keeps `spyglass-rng` dependency-free, but means owning a hash implementation for no gain in speed or correctness.
- **The `md5` crate:** also small, but a different family from the `sha1` crate already in use, so it wouldn't share `digest`.
