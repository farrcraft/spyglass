# m26. Data packs in generation

- Status: Completed
- Milestone: [26. Data packs in generation](../../roadmap/completed/26-data-packs.md)
- Started: 2026-11-11
- Completed: 2026-11-13

## Goal

Every exit criterion of milestone 26: a save's worldgen packs, and packs given for a seed, are applied as the game applies them, checked against the game with the same packs loaded, and `world info` warns only about what spyglass can't apply.

## Approach

A bundle is laid out as a data pack already (`data/<namespace>/worldgen/**`, tags, templates; [bundle-format.md](../../specs/bundle-format.md#layout)), and every loader reads it through four `Bundle` methods: `resource(registry, id)`, `registry_ids(registry)`, `structure_template(id)` and `report(path)`. Tags are read through `resource` too, by the biome, density and structure loaders alike. So packs can be layers inside `Bundle`, below which nothing changes.

### The game's rules, which this follows

To confirm in the 26.3 bytecode before relying on them, and record in a new spec, `data-packs.md`:

- **Order.** `Data.DataPacks.Enabled`, lowest first: `vanilla`, then feature packs, mods' packs and `file/<name>` packs (folders and zips in `datapacks/`), each later one over the earlier (`PackRepository`, `MultiPackResourceManager`).
- **Files.** For a resource id, the highest pack that has the file wins; lower ones are hidden. A pack's `pack.mcmeta` `filter.block` patterns hide matching files of the packs below it.
- **Overlays.** `pack.mcmeta` `overlays.entries`: folders applied over the pack's root when their format range covers the version's data pack format, later entries over earlier ones. The bundle needs the version's data pack format, which the extractor can read from the jar's `version.json` (`pack_version`).
- **Tags** merge instead (`TagLoader`): each pack's file for a tag adds its `values`, in pack order, and `"replace": true` drops what lower packs gave. A value with `"required": false` that doesn't resolve is skipped.
- **Format.** A pack whose format range doesn't cover the version is still loaded by a world that enables it (with a warning in the game's UI), so spyglass applies it too and says so.
- **Registries.** A pack's new ids (biomes, structures, structure sets, features, density functions) join the registry: `registry_ids` returns the union.

### Crates

- **`spyglass-data`**: a `Pack` (a folder or zip, its `pack.mcmeta`, overlays and filter), and `Bundle::with_packs(packs)` returning a bundle whose four readers look through the layers top-down, merging tags. The manifest's file list stays the vanilla one; a layered bundle has its own identity (the bundle's plus each pack's path and content hash) for caches and `World` ids. Zips are read with limits, since packs are untrusted input as saves are ([world-saves.md](../../specs/world-saves.md#limits)): entry count, sizes and paths checked, no entry outside `data/` or the overlays read.
- **`spyglass-extract`**: the data pack format in `bundle.json`; the jar's built-in feature packs (`data/minecraft/datapacks/<name>/`) into the bundle, so a save enabling one gets it.
- **`spyglass`**: `World::open_with_packs` and `Game::open_with_packs`; `SaveReport::open_world` and `open_game` apply the save's enabled packs, in order. `SaveWarning::DataPack` narrows to packs spyglass can't apply: missing, a mod's, or holding a worldgen type spyglass doesn't load. `BundleInfo` or `SaveReport` lists the packs applied.
- **Unsupported types.** Loading is lazy in places; a pack's worldgen files are all loaded once when the world opens (density functions, noises, features, structures, biomes), so an unsupported type is found up front and reported with its file, not met mid-query.
- **The map:** a pack's new biomes get the hashed colour [ADR 0018](../../adr/0018-biome-palette.md) already gives unknown ids.
- **Front ends:** `--world` applies the save's packs; `--pack <dir|zip>` (repeatable, lowest first) applies packs for a seed; `world info` lists what it applied and what it couldn't. The viewer applies a save's packs and lists them in the save banner; a pack picker for seeds is a stretch task.

### The oracle

The harness already loads packs over vanilla (`--pack`, for [the test pack](../../specs/oracle.md#the-test-pack)), into probe and structure files under `<version>+testpack`. This generalizes it:

- every harness command takes `--pack` (repeatable), and `cargo xtask oracle <command> --pack <dir> --name <label>` writes into `oracle-out/<version>+<label>/`, with `cargo xtask oracle-test --game-version <version>+<label>` comparing a world opened with the same packs;
- the test pack's merged bundle in `target/oracle/bundles/` goes away: its comparisons open the plain bundle with the pack applied, so the overlay is what's checked.

### Fixture packs

Vanilla files can't be committed ([ADR 0008](../../adr/0008-game-derived-test-values-stay-local.md)), and a pack that overrides a noise or the noise settings must start from vanilla's. So the overriding fixture is generated, not committed: `cargo xtask oracle fixture-pack` writes `target/oracle/packs/overrides/` from the bundle, changing a noise's amplitudes, a density function's constant, the noise settings' sea level and an ore's count, a placed feature's count, and a biome tag's members, with a `pack.mcmeta`, an overlay and a `filter` so those paths are exercised. The program that writes it is committed; its output is spyglass's numbers on top of the user's own extracted files.

For the widely used pack, a candidate is chosen by what spyglass supports: a terrain pack such as Tectonic or Terralith, downloaded by hand into a gitignored folder. Its result, as far as it agrees and what it uses that spyglass doesn't support, goes in the spec.

### Decisions for an ADR

Packs as layers inside `Bundle`, not merged bundles on disk; the generated fixture pack; what "applied" means for a pack whose format doesn't match.

## Tasks

- [x] Confirm the game's rules in the 26.3 bytecode (`PackRepository` order, `FilterSection`, `OverlayMetadataSection` and its format ranges, `TagLoader` merging and `replace`, what a mismatched format does), and write `data-packs.md`
- [x] The extractor: the data pack format in `bundle.json`, and built-in feature packs into the bundle (a bundle format change: bundle-format.md, and a re-extract)
- [x] `Pack` (folders and zips, `pack.mcmeta`, overlays, filters) with limits, and unit tests on packs built in the test
- [x] `Bundle::with_packs`: layered `resource`, `registry_ids`, `structure_template` and `report`, merged tags, and an identity for caches
- [x] A fuzz target for pack zips and `pack.mcmeta` (`cargo xtask fuzz pack`; 60 s, 8,567 runs, no crash)
- [x] ADR: data packs as layers ([0072](../../adr/0072-data-packs-as-layers.md))
- [x] `World::open_with_packs`, `Game::open_with_packs`, and every pack worldgen file loaded once on open, unsupported types reported with their file
- [x] Saves: `SaveReport::open_world`/`open_game` apply the enabled packs; `SaveWarning::DataPack` narrowed; what was applied in the report and in `world info` (text and JSON)
- [x] The oracle: `--pack` on every harness command, `--name`, `oracle-test` for `<version>+<label>`; the test pack moved onto the overlay
- [x] `cargo xtask oracle fixture-pack`, and every comparison run with it: matching with the pack, and differing from vanilla without it. For 26.3, all 59 match with the pack, `/locate`'s two included, and every comparison with the game fails without it; for 26.2 and 26.4-snapshots 5 to 8, all 56 or 57 match with the pack.
- [x] `--pack` for seeds in the CLI
- [x] The viewer: a save's packs applied and listed
- [x] A widely used worldgen pack, checked locally for a few seeds, the result recorded: Terralith 2.6.5 for 26.3, in data-packs.md
- [x] User guide: a data packs page, and `world info`'s new lines
- [x] CHANGELOG

## Verification

- **Unit tests** on packs built in the test: file replacement, filters, overlays chosen by format, tag merging with and without `replace`, unresolved optional values, zips, malformed `pack.mcmeta`, oversized and escaping zip entries.
- **The fixture pack** through every oracle comparison (probe, biomes, structures, locate, spawn, heights, blocks, features, loot), with the pack and without: equal to the game with it, and different from vanilla somewhere in each comparison the pack can change, so a pack silently ignored would fail.
- **The test pack** on the overlay instead of a merged bundle, its comparisons unchanged.
- **A widely used pack** for a few seeds, locally.
- **Saves:** a save built in the test with a `file/` pack applies it, and `world info` names it; one with a missing pack warns.
- `cargo xtask fuzz` on the new target; `cargo xtask check` and `cargo xtask oracle-test`.

## Durable outputs

- [x] `data-packs.md`: the game's rules, what spyglass applies, and the widely used pack's result
- [x] bundle-format.md: the data pack format and feature packs
- [x] world-saves.md: the narrowed warning, and what `world info` reports (and cli-json.md)
- [x] oracle.md: `--pack` and `--name`, the fixture pack and its 26.3 results, the test pack on the overlay
- [x] An ADR: data packs as layers
- [x] The user guide

## Notes

- 2026-11-11: drafted. The overlay goes in `Bundle` because all reading already does; tags are the only resource that merges rather than replaces.
- 2026-11-11: the bytecode confirmed the rules, with three corrections to the draft. Filter patterns match anywhere in the id (`asPredicate`), not the whole id. The last listed overlay wins over earlier ones, and the root is lowest. A pack whose `pack` section fails validation still loads, of unknown compatibility; one with no `pack.mcmeta`, or broken overlays, isn't created at all. Feature pack ids are bare (`trade_rebalance`); none has worldgen files in 26.3, but `trade_rebalance` changes chest loot, which spyglass computes.
- 2026-11-11: crates differ from the plan. Opening folders and zips is in `spyglass-world` (`open_pack`), not `spyglass-data`, since ADR 0016 keeps archive code out of the data crate. `spyglass-data` has the types, metadata and layering, and a `PackReader` that keeps the limits. So `spyglass-oracle` depends on `spyglass-world`, whose row moved above it in the crate table.
- 2026-11-11: "every pack worldgen file loaded once on open" became: a world with packs loads everything that's otherwise lazy (pools, block settings, features, loot) on opening. An error there fails the open only if the bundle without the packs loads cleanly, so a bundle missing a part (the fixtures, old extractions) behaves as before. The loaders have no per-file entry points, and a file no dimension uses can't change answers.
- 2026-11-11: the harness loaded `--pack` folders as bare `PathPackResources`, which skips overlays and can't read zips. It now goes through `Pack.readMetaAndCreate` and `Pack.open`, as a world's packs do, so the test pack needed a `pack.mcmeta` (formats 82 on), and so did the no-carvers pack.
- 2026-11-11: the fixture pack for 26.3. The harness's biome writer refused `+` in version names; fixed. Under the pack, seed −1's first stronghold ring position is tie-dependent, and containers were answered in spyglass's chunk only, as certain; fixed with `Starts::possible_starts_in`, which gives a ring's alternatives as candidates (also used for the structure pieces features read). Results in oracle.md. One outpost chest's loot seed is still off: 2163 containers, 1 difference.
- 2026-11-12: the fixture pack for 26.2. Its dungeon table writes `rolls` as `3.0`, which the pack's writer refused; fixed. The containers harness then crashed in the game: a sculk patch read two chunks out, beyond the 3 × 3 it builds, where the game's `FEATURES` step has its neighbours only at `STRUCTURE_STARTS`, so no answer there is the game's. Such a chunk is now left out. The two-dungeon test needs feature files for the seeds the pack's search finds; `oracle features --seed` replaces the folder, which had left 26.3's with only those, so the fixture pack's golden set now writes them with the default seeds.
- 2026-11-12: the outpost chest. The pack's terrain moved the outpost, and at the new position `watchtower_overgrown`'s copy of the chest survived its `outpost_rot` (integrity 0.05), so the game drew twice and the second chest replaced the first. A block with NBT placed over a container now replaces it (or makes it uncertain, if it might not be placed). 56 of 56 with the pack; plain 26.3 still passes.
- 2026-11-13: snapshot 5 with the fixture pack: 57 of 57. `oracle locate` takes `--pack` and `--name`: each new world gets the packs in `datapacks/`, enabled in order with `initial-enabled-packs`. For 26.3 with the fixture pack, about half of each seed's 120 answers and most map targets differ from vanilla's.
- 2026-11-13: Terralith 2.6.5 (the data pack edition, in a gitignored `packs/`) found two faults. Whether a biome, tag or structure exists was looked up in the bundle's manifest only (`Bundle::has_resource` now looks through the packs). Surface rules and noise settings read only 26.2's `{"Name", "Properties"}` states; features already read 26.3's `{"id", "properties"}`, and now all three share `codec::block_state`. What's left is unsupported, not wrong: 290 of its 300 templates are from older data versions (2730 to 4325), which the game data-fixes and spyglass doesn't, and feature forms such as a `weighted` filling provider. Either refuses every dimension, since a world with packs loads every pool, feature and loot table on opening.
- 2026-11-13: Terralith, continued. Its carver mismatches near the world edge were a fault in carvers: the game keeps one random per chunk and reseeds it per carver, so its spare `nextGaussian` value carries over; spyglass made a fresh random each time (`LegacyWorldgenRandom` now keeps it). Terralith's canyons draw `clamped_normal` heights, which vanilla's don't. In the harness, the run without carvers took vanilla's biomes over the pack's (now built from the layered bundle), and golden headers named the variant after the pack's file (now `-Dspyglass.oracle.version`); the probe test parser refused `normalize` other than true. With those fixed, everything below structures agrees; the full `oracle-test` run passes every other version and variant.
- 2026-11-11: every local bundle re-extracted (26.2, 26.3, 26.4-snapshot-5 to 8) with its pack format. The test pack's comparisons for 26.4-snapshot-8 ran on the overlay; `dimensions` needed the variant's packs and base version.
