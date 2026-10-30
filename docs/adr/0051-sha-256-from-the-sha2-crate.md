# 0051. SHA-256 from the sha2 crate

- Status: Accepted
- Date: 2026-10-29

## Context

A block's biome isn't its quart's. `BiomeManager.getBiome` picks among the eight quarts around a block with a "fiddle" seeded from `BiomeManager.obfuscateSeed(seed)`, which is the first eight bytes of the SHA-256 of the seed's eight bytes (`Hashing.sha256().hashLong(seed).asLong()`, from Guava). The material rule's biome and temperature conditions read biomes this way, so `spyglass-blocks` ([ADR 0050](0050-blocks-in-their-own-crate.md)) needs SHA-256.

The workspace already takes MD5 and SHA-1 from RustCrypto's crates ([ADR 0009](0009-md5-from-the-md-5-crate.md)), which share the `digest` 0.11 base.

## Decision

We will use RustCrypto's **`sha2`** crate (`0.11`), through `[workspace.dependencies]`, in `spyglass-blocks`. Its port of `obfuscateSeed` is tested against known SHA-256 vectors, and against the game through the block golden files.

## Consequences

- It's the same family as `md-5` and `sha1`, so it shares `digest` 0.11 and the tree barely grows. Its license (MIT or Apache-2.0) is allowed in `deny.toml`, and its MSRV (1.88) is below the workspace's.
- Only `spyglass-blocks` and what depends on it take it on.

## Alternatives considered

- **Hand-written SHA-256:** about 100 lines, tested against FIPS 180-4's vectors, with no dependency. It's the same trade ADR 0009 declined for MD5: owning a hash implementation for no gain.
