# 0010. Typed loading of worldgen JSON

- Status: Accepted
- Date: 2026-10-25

## Context

A version bundle is the game's worldgen JSON, byte for byte ([ADR 0006](0006-bundle-format-raw-json-tree.md)). Every layer above `spyglass-data` has to turn some of it into typed Rust structures: density functions and noise parameters now, then biome parameter lists, structures, structure sets, tags and template pools. How that's done gets copied by every later layer, so it should be decided once.

What makes it awkward:

- **The game's codecs accept several shapes for one field.** A density-function argument can be a number (a constant), a string (a reference to a named function, `"minecraft:overworld/continents"`) or an object with a `type`. A noise field can be a string id or an inline noise definition. A spline value can be a number or a nested spline. Biome parameters can be a number or a `[min, max]` pair. Fields have defaults that live only in the codecs: `normalize` defaults to `enabled`, `base_amplitude` to 1.0.
- **References cross files and registries.** The overworld's noise settings plus the density functions they reach are 62 files using 33 types in 26.3. Named functions refer to other named functions and to noises, and the same function is referenced from several places.
- **Numbers must parse exactly.** The game reads JSON numbers with `Double.parseDouble`, which is correctly rounded, then narrows where the codec says (`float` for most density-function constants in 26.3). `serde_json` by default parses floats with best-effort precision, which can be one ulp off. It's correctly rounded only with its `float_roundtrip` feature, which the workspace doesn't enable.
- **Layering.** `spyglass-noise` must not depend on `spyglass-data` (see [architecture](../architecture.md#crates)). Worldgen types belong to the layer that evaluates them, not to the data crate.
- **Errors are user-facing.** A future version or a user's datapack will contain types spyglass doesn't support yet. That should fail when the bundle is loaded, saying which type and where, not as a panic or a silent wrong answer during evaluation.
- **Datapacks are a goal.** Users' world saves can include worldgen datapacks, which use the same JSON and the same shorthands.

## Decision

We will load worldgen JSON as follows:

1. **`spyglass-data` stays type-agnostic.** It gives access to a bundle's resources by registry and id (`density_function`, `minecraft:overworld/continents` → `data/minecraft/worldgen/density_function/overworld/continents.json`) and to reports by path, as `serde_json::Value`. It knows the datapack layout, not the meaning of any file.
2. **Each layer parses its own types**, in the crate that evaluates them. `spyglass-density` parses density functions, noise settings, and noise parameters into `spyglass_noise::NormalNoiseParameters`; `spyglass-biome` parses parameter lists, and so on. Lower crates such as `spyglass-noise` take plain Rust structs and never see JSON.
3. **Parsing is a hand-written walk over `serde_json::Value`**, written to mirror the game's codec for each type: the same field names, the same defaults (cited from the codec), the same accepted shapes, the same range checks. Serde derive is fine for simple fixed-shape files (such as `bundle.json`), but not for codec types.
4. **References are resolved at load time and shared.** A named reference is loaded once per registry and id, and every use points at the same node (`Rc`/`Arc` or an arena index, the layer's choice). Cycles are an error. Noises are resolved the same way. Seeding stays separate: a loaded graph is seed-independent, and binding to a seed is a later step.
5. **Errors name the type and the path.** A load error carries the resource id and a JSON path to the offending value (for example `minecraft:overworld/offset: argument2.spline.points[3].value: unsupported type "minecraft:foo"`), and covers unknown types, missing fields, wrong shapes and out-of-range values. Loading never panics on bad input.
6. **Floats parse correctly rounded.** The workspace enables `serde_json`'s `float_roundtrip` feature. Each type narrows to `f32` only where the game's codec does, as its spec states.

## Consequences

- One loading style for every layer. Each type's parser can be read next to the game's codec, and its spec lists the fields and defaults.
- The data crate doesn't change when a layer adds types, and lower crates stay free of JSON.
- Datapacks can reuse the same loaders later: they only change where the `Value`s come from.
- Hand-written parsing is more code than serde derive, and every field and default must be copied from the codec by hand. A missed default is a parity bug. The specs and the oracle's probes are the check on that.
- `serde_json::Value` keeps the whole parsed document in memory. For a bundle's few megabytes of JSON that doesn't matter.
- Enabling `float_roundtrip` makes float parsing about twice as slow, which only affects loading. It also changes any float spyglass already parses: the biome report values become correctly rounded too, which the quantization rule in [data-sources.md](../specs/data-sources.md#climate-parameter-values) already assumes.
- Resolving references at load time means a bundle with an unsupported type somewhere unused still fails if that type is reachable from what's loaded. Layers load only what they need, starting from the router, so unrelated files aren't touched.

## Alternatives considered

- **Serde derive with untagged enums and custom deserializers:** less code for simple types, but the codec's number/string/object unions and context-dependent defaults turn into custom `Deserialize` implementations anyway. Serde's untagged-enum errors also say "data did not match any variant" without saying where or why, which fails the error requirement.
- **Typed structures in `spyglass-data`:** one crate that knows every format. Rejected: the data crate would depend on every layer's types, or duplicate them. Either way every worldgen change touches it, and it breaks the rule that types live with the code that evaluates them.
- **Resolving references lazily during evaluation:** simpler loading, but unsupported types and broken references would surface in the middle of a search. A lookup by id on every evaluation is also a cost in the hot path.
- **A schema-driven generator** (codec descriptions → Rust parsers): appealing given hundreds of types eventually, but there's no machine-readable source of the game's codecs to generate from. Revisit if the hand-written parsers become repetitive enough to justify a macro.
