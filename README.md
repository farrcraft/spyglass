# spyglass

A Rust-native, offline seed-map toolkit for Minecraft: Java Edition, covering biome maps, structure finders and seed search. It aims for bit-exact output on current versions.

> Status: scaffolding only. Nothing works yet.

Not affiliated with or endorsed by Mojang or Microsoft.

## Why

Existing seed-map tools either hand-port per-version tables and fall behind new releases, or are closed-source. spyglass keeps the **algorithms** in Rust and **extracts per-version parameters automatically** from the game's own jar. Supporting a new version should mean running the extractor, diffing the output and fixing the code that actually changed, not re-porting tables by hand.

Mojang's data and code are never committed or redistributed. The extractor runs locally against a server jar you download yourself.

## Quick start

```sh
# Windows (PowerShell)
./scripts/setup.ps1

# macOS / Linux
./scripts/setup.sh
```

Then:

```sh
cargo build
cargo test
cargo run -p spyglass-viewer
```

See [docs/getting-started.md](docs/getting-started.md) for the full walkthrough, including installing Rust by hand.

## Crates

| Crate | Role |
|---|---|
| `spyglass-rng` | Bit-exact ports of the game's RNGs. |
| `spyglass-noise` | Noise primitives. |
| `spyglass-data` | Version bundles, the manifest client and the extractor CLI. |
| `spyglass-density` | Loads and evaluates density-function graphs. |
| `spyglass-biome` | Climate sampling and biome lookup. |
| `spyglass-terrain` | Heightmaps and surface rules. |
| `spyglass-structure` | Structure placement and validity checks. |
| `spyglass-search` | Parallel seed search. |
| `spyglass-viewer` | Native map app. |

See [docs/architecture.md](docs/architecture.md) for how they fit together.

## Documentation

- [Getting started](docs/getting-started.md)
- [Architecture](docs/architecture.md)
- [Specs](docs/specs/)
- [Roadmap](docs/roadmap/README.md)
- [Development process](docs/development-process.md)
- [Decision records](docs/adr/)
- [Research](docs/research/)

## License

[MIT](LICENSE). This covers spyglass's own code only. Minecraft's code and data stay under Mojang's terms, which is why spyglass never ships them.
