# 0023. Dimensions in the public API

- Status: Accepted
- Date: 2026-10-26

## Context

[ADR 0014](0014-public-api-world-and-session.md) made a `World` one dimension of a bundle, and `World::open` has only ever opened the Overworld of the Default preset. Milestone 08 adds the Nether and the End, and the user-facing questions span dimensions in three ways:

- **One dimension at a time.** Locate, heights, maps and saves usually ask about one dimension: "the nearest fortress from here in the Nether".
- **Across dimensions for one seed.** Search constraints such as "a fortress within 100 Nether blocks of the Overworld spawn's portal position" need two dimensions' answers for the same seed ([ADR 0024](0024-dimensions-in-the-search-language.md)).
- **Saves.** A save has all three dimensions, and its player may be in any of them.

The forces:

- **Each dimension's seed-independent data stands alone.** It has its own noise settings, biome source, structure sets and terrain settings, and costs about the same to load as the Overworld's. A search that needs only the Overworld shouldn't pay for the Nether.
- **Borrowing.** Sessions borrow their world (`Session<'w>`), and searches share one world across threads, so worlds must outlive their sessions and be `Send + Sync`.
- **Existing users.** The CLI, the viewer, the benches and every test call `World::open` and expect the Overworld.

## Decision

We will keep `World` as one dimension and add a bundle-wide type over it.

- `Dimension` is `Overworld`, `Nether` or `End`: the Default preset's three dimensions, by their ids.
- `World::open_dimension(dir, dimension)` opens one dimension of a bundle. `World::open(dir)` stays the Overworld.
- `World::dimension` returns the dimension, and `coordinate_scale` its dimension type's coordinate scale, from data.
- `Game` (one bundle) opens each dimension's `World` the first time it's asked for, keeps it, and hands out `&World`. It resolves queries that may span dimensions. `Game::session(seed)` gives a session over every dimension, binding each dimension's `Session` on first use.
- Search runs on a `Game`. `World::search` stays for queries in its own dimension, and rejects others.
- A save's report opens any dimension, or the whole `Game`. Saves are assessed per dimension, since each has its own generator.

## Consequences

- Single-dimension code is unchanged. Front ends that show one dimension take a `Dimension` and call `World::open_dimension`.
- Nothing is loaded for a dimension nobody asks about: opening a `Game` costs only reading the bundle's manifest, and a Nether-only search never loads the End.
- `Game` keeps worlds in `OnceLock`s, so it's `Send + Sync`. Its sessions borrow it, as `Session` borrows `World`.
- Two ways to open a world: `Game::world` shares one load; `World::open_dimension` doesn't. Front ends that switch dimensions (the viewer) use a `Game`.
- Only the Default preset's three dimensions exist. Datapack dimensions and other presets stay out, as before.

## Alternatives considered

- **One `World` holding all three dimensions:** one type fewer, but every query would need a dimension argument, opening a world would load or lazily guard three sets of data everywhere, and ADR 0014's one-dimension contract, which every test relies on, would change.
- **Separate `World`s and no bundle-wide type:** front ends would juggle three worlds, and cross-dimension search would need a new parameter list taking several worlds and checking they share a bundle and seed. `Game` is that check, in one place.
- **Dimensions as strings:** open-ended, but nothing beyond the three vanilla dimensions can be answered, and a typo would be a runtime error instead of a compile error.
