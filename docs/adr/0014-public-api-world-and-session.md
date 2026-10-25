# 0014. Public API: a loaded world and seed-bound sessions

- Status: Accepted
- Date: 2026-10-25

## Context

spyglass is library-first ([architecture.md](../architecture.md#front-ends)): the CLI, the GUI and Rust users all go through the `spyglass` crate, and no front end holds worldgen logic. Milestone 03 adds that crate and the first front end, the `spyglass` command. Its shape will be hard to change once the viewer and scripts depend on it.

The forces:

- **Two costs, two lifetimes.** Loading a dimension from a version bundle (density functions, the biome parameter list and search tree, structure sets, templates) is seed-independent and takes a while. Binding a seed (noises, samplers) is cheap, but some per-seed work is slow and only sometimes needed: the 128 stronghold ring positions take about 2.4 s. A seed search binds thousands of seeds against one loaded dimension.
- **The lower crates borrow.** `Biomes<'s>`, `Placements<'s>` and `Starts<'a>` borrow the seed-independent data. An object owning both halves would be self-referential.
- **Answers aren't always single values.** Tied biome cells, platform-dependent trig results ([ADR 0012](0012-java-trig-results-that-depend-on-the-platform.md)) and structure starts that need terrain (until milestone 07) mean the honest answer is sometimes "one of these". Hiding that would break the bit-exact promise silently.
- **Internal indices aren't stable.** `BiomeId` and structure indices depend on load order; game ids (`minecraft:plains`) are what users and scripts know.
- **Scripts depend on `--json`.** Its schema must be versioned and documented, and must not change because a Rust type gained a field.

## Decision

We will expose two types from `spyglass`:

- **`World`**: one dimension of one version bundle, loaded once (`World::open(bundle_dir)`, the Overworld of the Default preset for now). Seed-independent and shareable.
- **`Session<'w>`**: a `World` bound to a seed (`world.session(seed)`), borrowing it. Creating one is cheap; slow per-seed data (ring positions) is computed on first use and kept. Queries are methods: `biome_at`, `locate_structure`, `locate_biome`, `structures_in`, `is_slime_chunk`, `slime_chunks`, `spawn_origin`.

And these rules for the API:

- Results name things by game id, never by internal index.
- Results carry their uncertainty: a tied biome lists its tied set; a locate answer is a list of hits in the game's search order, each `certain` or not; platform- and tie-dependent results are flagged. Front ends decide how to show it; the library never silently picks.
- One error type, `spyglass::Error` (`thiserror`), for bad input and bad bundles. No panics on user input.
- Seeds from text follow the game's world-creation rule (`WorldOptions.parseSeed`), in `spyglass::parse_seed`.
- The library doesn't derive `serde` traits. The CLI owns its `--json` output, specified and versioned in [cli-json.md](../specs/cli-json.md).
- Front ends contain no worldgen logic; a missing query is added to `spyglass`, not computed in a front end.

## Consequences

- A seed search or a viewer switching seeds reuses one `World`; per-seed cost is binding plus whatever queries need.
- `Session<'w>` borrows, so a long-lived front end holds the `World` (e.g. in an `Arc` or a field) and creates sessions from it. That's a little more ceremony than one owned object.
- Every query result has a type of its own in `spyglass`, converted from the lower crates' types. More code than re-exporting, but the lower crates can change freely.
- Uncertain answers make callers handle lists and flags. That's the cost of not hiding what the game itself leaves open.
- The JSON schema is a second description of results, kept in step with the API by hand and by the CLI's tests.

## Alternatives considered

- **One owning `Session` (bundle + seed + dimension)**, as first sketched: needs self-referential storage or `Arc`s through every lower crate, and reloads the dimension per seed unless it caches behind the user's back.
- **Re-export the lower crates' types:** less code now, but `BiomeId`s and indices leak, and every internal change becomes an API change.
- **`serde` on library types, JSON = the types:** convenient, but couples the scripted format to Rust refactors and adds `serde` to every result type.
- **A single best answer per query:** simpler to use, but wrong without warning where the game's answer depends on terrain, ties or the platform.
