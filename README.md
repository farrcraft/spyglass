# spyglass

A local-only seed explorer for Minecraft: Java Edition. Describe what you want in a world, and spyglass searches for the seeds that fit best, shows them on a map, and finds where things are in worlds you already play. Output aims to match the game exactly on current versions.

> Status: scaffolding only. Nothing works yet.

Not affiliated with or endorsed by Mojang or Microsoft.

## Goals

- **Constraint search.** Give spyglass constraints like "at least 3 witch huts within 500 blocks of spawn, a mushroom fields biome within 2,000, a village and an ancient city nearby", and it searches seeds in parallel and **ranks** the best matches. Constraints are required (a seed must meet them) or preferred (a seed scores higher the better it meets them).
- **Visual preview.** Open any seed on a pannable, zoomable map, and choose what to render: biomes, chosen structure types, slime chunks, spawn, or highlights of specific biomes.
- **Your existing worlds.** Point spyglass at a local world save and it reads the seed, version and world settings, then answers "where is the nearest stronghold, ancient city or mushroom island from here?" for that world, not just for theoretical seeds.
- **All three dimensions:** Overworld, Nether and End.
- **Library first.** Everything is a Rust library. A command-line tool (JSON output, for scripts) and the GUI are both thin front ends over the same API, so anything the GUI can show, a script can compute.
- **Local only.** No server, no account, no telemetry. The only network access is downloading the game's own server jar from Mojang, which the extractor does once per version.

## Why

Existing seed tools either hand-port per-version tables and fall behind new releases, or are closed-source web apps. spyglass keeps the **algorithms** in Rust and **extracts per-version parameters automatically** from the game's own jar. Supporting a new version should mean running the extractor, diffing the output and fixing the code that actually changed, not re-porting tables by hand. Correctness is checked against the real game, not assumed.

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
cargo xtask doctor   # is this machine ready?
cargo build
cargo test
```

See [docs/getting-started.md](docs/getting-started.md) for the full walkthrough, including installing Rust by hand.

## Crates

| Crate                | Role                                                        |
| -------------------- | ----------------------------------------------------------- |
| `spyglass-rng`       | Bit-exact ports of the game's RNGs.                         |
| `spyglass-noise`     | Bit-exact ports of the game's noise.                        |
| `spyglass-data`      | Version bundles, the manifest client and the extractor CLI. |
| `spyglass-density`   | Loads and evaluates density-function graphs.                |
| `spyglass-biome`     | Climate sampling and biome lookup.                          |
| `spyglass-terrain`   | Heightmaps and surface rules.                               |
| `spyglass-structure` | Structure placement and validity checks.                    |
| `spyglass-search`    | Parallel, ranked seed search.                               |
| `spyglass-viewer`    | Native map app.                                             |

More are planned for the public API, the command-line tool and reading world saves. See [docs/architecture.md](docs/architecture.md) for how they fit together.

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
