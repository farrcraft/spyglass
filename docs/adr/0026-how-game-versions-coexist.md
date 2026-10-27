# 0026. How game versions coexist

- Status: Accepted
- Date: 2026-10-26

## Context

spyglass supported one game version, 26.3. It now has to support several side by side: 26.2, 26.3, and 26.4 as it moves through its snapshots, with more to follow, and every one must stay bit-exact against the oracle. [Architecture](../architecture.md#code-vs-data) splits algorithms (Rust) from parameters (extracted per version). That split assumed algorithms rarely change between versions. A bytecode survey of every ported class across 26.2, 26.3 and 26.4-snapshot-5 tested that assumption. The survey's results are drafted in the specs' _Versions_ notes ([noise](../specs/noise.md), [density functions](../specs/density-functions.md), [biome lookup](../specs/biome-lookup.md), [terrain](../specs/terrain.md), [structure placement](../specs/structure-placement.md), [spawn](../specs/spawn.md)). Versions differ in three ways:

- **Format changes that name themselves.** The data shows which form it is. Examples: `argument1` or `left`, `cache_2d` or `cache`, `firstOctave` or `base_octave`, `interpolated` or `upscale`, and template palettes keyed `Name` or `id`.
- **Behavior the data doesn't show.** These are small, contained changes. 26.4's climate parameter distance becomes half-open. Its heights count only solid blocks for most heightmaps. A NaN spline coordinate returns NaN.
- **A different algorithm family.** 26.2 evaluates density the 1.21 way. Everything is `f64`. There is no volume arithmetic. Caches and interpolation exist only in `NoiseChunk`, and `flat_cache` is quantized to a quart corner inside the chunk's window. 26.3 rewrote this in `f32` with volume evaluation ([ADR 0022](0022-terrain-crate-and-volume-evaluation.md)). The two agree in shape, but not one result in a thousand is bit-equal.

Snapshots arrive weekly, so adding a version has to be cheap. Support is permanent, snapshots included ([ADR 0028](0028-supported-versions-and-ci.md)). Versions only accumulate, so whatever distinguishes them has to stay readable as their number grows. A snapshot can also change a behavior that the next snapshot changes back.

## Decision

We will support versions as follows:

1. **Loaders dispatch on the data's own shape.** They dispatch on type ids and field names, never on the version. One loader reads every supported version's form of a file. A form from no supported version stays an error that names the file and the JSON path ([ADR 0010](0010-typed-loading-of-worldgen-json.md)).
2. **Behavior the data doesn't show is a named switch.** All switches live in one table, `spyglass_data::versions`, as a `Behavior` struct of named fields. Each field lists the data-version ranges it applies to. A range is usually open-ended ("from 5125 on"), but it can be closed, for a change a later snapshot reverts.
   - Code reads a switch's field, never a data version.
   - Lower crates, which don't depend on `spyglass-data`, take the field they need as a plain parameter.
   - Each switch's doc comment names its spec section, and each spec's _Versions_ note names the switch.
   - `cargo xtask version-diff` prints the switches for any bundle.
3. **One evaluator per algorithm family.** Most versions share one code path per algorithm. Where the survey shows a family that differs throughout and not at a few points, that family gets its own evaluator. The only such family now is 26.2's `f64` density evaluation and its `NoiseChunk`.
   - Each family is a module of the crate that owns the layer: `spyglass-density`, and `spyglass-terrain` for the chunk path.
   - The family is chosen by the data's shape when a router is bound: a 15-field router with `noise.size_horizontal` is 1.21-family.
   - Everything else is shared: the RNG, the noise primitives where they agree, the loaders, and every layer above density.
   - Behavior that varies between families sits behind the same interface (sampling a router, a column, a volume), so callers don't know which family they hold.
4. **Specs describe each algorithm once.** Where a version differs, the spec has a _Versions_ note. A whole family gets its own section.
5. **Every version stays accounted for.** Dispatch arms, switches and families are never deleted while a supported version uses them, which is forever. Each arm's doc comment and each switch's ranges say which versions use it, and `version-diff` prints any bundle's switches. A test that every supported bundle loads and binds keeps old arms working.

## Consequences

- **Snapshots stay cheap.** A new snapshot costs a loader arm or a switch, located by `version-diff` and a spec diff. A change that doesn't fit either is the signal to stop and rethink: the plan records a retro, and the fix becomes a task or an ADR.
- **Switches are visible.** They're named and greppable, and a test pins each one with a hand-worked case that needs no game data. Each switch is a branch in code that already runs in hot loops. Switches are read at bind time where possible: a sampler or a column is built for one behavior, not branching per sample.
- **The second family costs a lot.** It is a second density evaluator: types, `NoiseChunk` caches, interpolation, `BlendedNoise`, the End islands and the aquifer's inputs. It needs its own probe coverage. A fix or optimization to one family doesn't reach the other. This is the price of supporting 26.2, and it's paid only once per family, not once per version. The same family covers releases before 26.2 that share its format, if they're ever wanted.
- **Code only grows with versions.** Switches, dispatch arms and families accumulate and are never removed. That makes the switch table's readability, and grouping switches by the feature they belong to, ongoing work rather than a cleanup to do once.
- **Datapacks follow version support.** A pack written for a supported version loads, because loaders accept every supported form.
- **The specs grow _Versions_ notes.** These are drafts until each version's oracle comparisons pass.

## Alternatives considered

- **Per-version modules or crates for everything:** each version a copy of the code. Copies drift. A bug fixed in one stays in the others, and most code is identical across versions. Used only where a whole family differs (decision point 3).
- **Converting old data to the new format at extraction:** one loader, one evaluator. But the 26.2 evaluator isn't 26.3's with old field names. It computes in other types, in another order, with other caches. Converting the data would give 26.3's results for 26.2's seeds: wrong answers, labelled correct.
- **Version checks where they're needed** (`if data_version >= 5100`): the least code at first. But there's no single place to see what a version changes, a reverted change needs a pair of comparisons wherever it's checked, and the numbers carry no meaning when read.
- **One evaluator with switches for every 26.2 difference:** avoids a second family. But `f64` against `f32`, the evaluation order and the cache semantics differ at nearly every step. The switches would outnumber the code and make the hot path for 26.3 and 26.4 slower and harder to read.
