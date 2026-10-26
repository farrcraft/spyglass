# 0016. The extractor in its own crate; `spyglass-data` reads only

- Status: Accepted
- Date: 2026-10-25

## Context

`spyglass-data` holds two kinds of code: reading a bundle (`Bundle`, the manifest, `codec`, `nbt`), which every worldgen crate needs, and building one (the version manifest client, the jar cache, the Java search, the data generator, the bundler and the `spyglass-extract` binary), which only the extractor and `xtask` need. Its dependencies are unconditional, so every library crate, the public `spyglass` API included, pulls in `ureq` (with `rustls` and `ring`), `zip`, `clap`, `anyhow`, `tempfile` and `directories`. `BundleError::Zip` puts a `zip` type in the error of every crate that reads a bundle.

That goes against the README's "local only" goal for library users and against the crate layering ([architecture.md](../architecture.md)), and it makes the viewer and any embedder compile a TLS stack they never use. It will get worse as the extractor grows (more versions, more data generator reports).

The loaders have a related problem. The JSON-loading helpers every loader shares (`LoadError`, the JSON-path tracker `At`, the field readers and `full_id`) live in `spyglass-density`, or are copied into `spyglass-biome` and `spyglass-structure`. Biome and structure loaders depend on the density crate partly for its error type.

## Decision

We will:

- Move building bundles into a new crate, **`spyglass-extract`**: the manifest client, cache, Java search, data generator, bundler, `extract`, the writing half of `bundle` (`write`, `Sources`, the jar copy) and the `spyglass-extract` binary. It owns `ureq`, `zip` (for jars), `clap`, `anyhow`, `tempfile` and `directories`, with an `ExtractError` for its failures. `xtask` depends on it.
- Keep **`spyglass-data`** read-only: `Bundle`, `read_manifest`, `verify`, the manifest types, `codec` and `nbt`, with `serde`, `serde_json`, `thiserror`, `flate2` and `sha1` (for `verify`). `BundleError` keeps only reading errors.
- Put the shared loading helpers in **`spyglass_data::codec`**: `LoadError` and its kinds, `At`, the field readers, `full_id` and registry listing. Every loader uses them from there.

Nothing else may depend on `spyglass-extract`. The layering in [architecture.md](../architecture.md) gains it beside `xtask`, as tooling over the data crate.

## Consequences

- No network, CLI or archive-writing code is compiled into the library crates or the `spyglass` API. `cargo tree -p spyglass -e normal` shows this. `zip` is still there for reading zipped data packs in `spyglass-world`.
- The extractor command becomes `cargo run -p spyglass-extract`. The docs, the setup scripts, `xtask doctor`, `xtask linux-check` and anyone's shell history using `-p spyglass-data --bin spyglass-extract` must change.
- A bundle's `extractor` field records the `spyglass-extract` version, not the `spyglass-data` version. Both are the workspace version, so existing bundles are unaffected.
- Loader errors become a `spyglass-data` type, `spyglass_data::codec::LoadError`. Every crate that named `spyglass_density::LoadError` changes. Nothing is published, so there's no compatibility re-export.
- One more crate to keep in the layering table and the release story.

## Alternatives considered

- **Cargo features on `spyglass-data`** (`extract` off by default): smaller change, but `cargo build --workspace` and `cargo test --workspace` unify features, so the stack is still compiled and linked into every test binary, and the errors still mention `zip`. Feature-gated modules are also easy to break without noticing.
- **Moving the extractor into `xtask`**: the extractor is a user-facing command named in the README, and xtask is development tooling ([ADR 0003](0003-cargo-xtask-for-project-commands.md)).
- **A separate `spyglass-codec` crate for the loading helpers**: they're small and already sit next to `codec`'s number parsing, which every loader also uses. A crate of its own adds a layer without separating anything.
