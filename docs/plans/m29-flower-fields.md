# m29. Flower fields

- Status: In progress
- Milestone: [29. Flower fields](../roadmap/29-flower-fields.md)
- Started: 2026-11-15
- Completed:

## Goal

Answer which flower bone meal grows on grass at any block and height, and use that answer for a point, the map, the best height for an area, and a search.

This plan covers all four of the milestone's exit criteria:

- the flower at a block matches the oracle for every vanilla flower feature that picks by position;
- the viewer's flower layer and its best-height list;
- the search constraint, in a seed search and a search within one seed;
- the user guide, with a flower farm as its example.

It covers every supported version: 26.2, 26.3 and the 26.4 snapshots. The milestone names 26.3 and 26.4. 26.2 runs the same code and differs only in how its data spells the providers, which the bundle loader already deals with for other features.

## What the game does

These notes come from the 26.3 server jar, disassembled locally with `javap` (nothing from it is kept). 26.2 differs where it says so.

**Bone meal on grass** is `GrassBlock.performBonemeal`. It starts at the block above the grass and makes 128 tries. Try `i` walks `i / 16` random steps: x and z by −1 to 1 each, and y by `nextIntBetweenInclusive(-1, 1) · nextInt(3) / 2`. It gives up on the try if the walk leaves grass (`stopBonemealSpread`). Then `placeBonemealEffect` runs at the block the walk reached. If that block is air and inside the build height, the effect has two outcomes:

- With chance 0.125 (`nextFloat() < 0.125`) it makes a flower:
  1. It takes the biome at that block, `ServerLevel.getBiome`, which is `BiomeManager`'s zoomed biome, the one the F3 screen shows.
  2. It takes that biome's `getBoneMealFeatures()`: every configured feature reachable from the biome's placed features, nested ones included (`PlacedFeature.getFeatures`), that is in the tag `#minecraft:can_spawn_from_bone_meal`.
  3. If that list is empty, it stops. Otherwise it picks one feature uniformly (`Util.getRandom`) and places it at the block.
- Otherwise it places the `grass_bonemeal` placed feature.

**The flower features** in the tag are `simple_block` features: `flower_default`, `flower_flower_forest`, `flower_swamp`, `flower_plain`, `flower_meadow`, `flower_cherry`, `wildflower` and `flower_pale_garden`. Each places its `to_place` block state provider's state at the block. In 26.3 a provider can be named from the new `worldgen/block_state_provider` registry (`flower_plain`, `flower_meadow`, `flower_flower_forest`), or written inline. In 26.2 it's always inline, with `_provider` type names (`noise_threshold_provider`) and the older noise-parameter form.

**The providers vanilla flowers use:**

| Provider          | Used by                                                                              | Picks by position?                                                                                              |
| ----------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| A single state    | swamp (blue orchid), pale garden (closed eyeblossom)                                 | It's one flower.                                                                                                |
| `weighted`        | default (poppy 2, dandelion 1), cherry (pink petals), wildflower                     | No: odds only.                                                                                                  |
| `noise`           | flower forest: 11 flowers by one noise                                               | Yes, entirely.                                                                                                  |
| `noise_threshold` | plains, sunflower plains, deep dark, dripstone caves                                 | Partly: below the threshold, a random tulip; above it, a random high flower with `high_chance`, else dandelion. |
| `dual_noise`      | meadow: a slow noise picks how many and which flowers, the fast one picks among them | Yes.                                                                                                            |

**What decides a field's shape.** Every noise provider builds its noise from `WorldgenRandom(LegacyRandomSource(seed))` with the provider's own `seed` field (2345 in vanilla), not the world's. It samples the noise at `(x · scale, y · scale, z · scale)`. So **the flower a provider picks at a block is the same in every world**. Only the biome, which the world seed decides, says which provider applies. A field's shape changes with height through the noise's y, at about a block in 48 for the flower forest. The biome can change with height too: deep dark and dripstone caves, both underground, use the plains provider.

**Answers with odds.** Three kinds of randomness make an answer odds rather than one flower:

- a biome with more than one bone meal feature (meadow: `flower_meadow` or `wildflower`; birch forests: `wildflower` or `flower_default`), each with odds 1 in n;
- a `weighted` provider;
- `noise_threshold`'s random picks.

The answer at a block is the list of flowers it can grow, each with its chance given that the effect makes a flower. Where the biome has no bone meal feature, the answer is "no flower". Where the biome lookup is tied ([biome-lookup.md](../specs/biome-lookup.md)), there's one answer per candidate biome.

## Approach

Four parts, each building on the one before: the flower at a block, the map and best heights, the search, and the front ends with the guide.

### The flower at a block

- **Loading** (`spyglass-feature`, beside `blocks.rs`'s `parse_simple_provider`):
  - The block state providers the flower features need: the single state, `weighted`, `noise`, `noise_threshold` and `dual_noise`, in 26.2's and 26.3's spellings. The 26.3 `block_state_provider` registry is read from the bundle, and data packs layer it like the other registries ([ADR 0072](../adr/0072-data-packs-as-layers.md)).
  - The feature tag `can_spawn_from_bone_meal`.
  - Each biome's bone meal features, gathered as `getBoneMealFeatures` gathers them, in the same order.
  - A provider the loader doesn't know fails only flower answers, not the bundle, as unsupported features do today.
- **Providers** (`spyglass_feature::flowers`, new): each provider's `getState` as the game computes it, with its random draws in the game's order, so the oracle can check it draw for draw. Beside that, each provider's odds at a block, worked out from its structure: the chance of each state, without drawing. The noises come from `spyglass_noise`'s `NormalNoise::create` over `WorldgenRandom(LegacyRandomSource(seed))`, built once per provider.
- **The API** (`spyglass`):
  - `Session::biome_at_block([x, y, z])`: the zoomed biome `ServerLevel.getBiome` gives. `spyglass-blocks` already has `BiomeManager`'s zoom.
  - `Session::flower_at([x, y, z]) -> FlowerAt`: the biome, its bone meal features, and the flowers with their chances, by candidate biome when tied.
  - The position is the flower's own block, one above the grass, as the game samples it.

### The map and the best height

- **The map:** `Session::flower_map(area, y) -> FlowerMap` gives each pixel's corner block's answer at `y`, as the underground layers do ([render.md](../specs/render.md#underground)). It also has a key of the flowers it shows. Biomes are looked up once per quart, and each provider's noise once per block.
- **The layer:** a flower layer in `Layers` (`Layers::flowers`) draws:
  - a block whose answer is one flower in that flower's colour;
  - a block with odds as a checker of its flowers, the way tied biomes are drawn;
  - a block without a flower left dimmed.

  With a highlighted flower, every block that can't grow it is dimmed, and blocks that can grow it only with odds are drawn lighter. Colours come from a built-in table keyed by block id, with the hashed colour as a fallback ([ADR 0018](../adr/0018-biome-palette.md)'s rule). The layer works at every scale: each pixel is one block's answer, and costs about as much as a biome.

- **The best height:** `Session::flower_heights(area, flower, y range, step) -> Vec<FlowerShare>` gives, for each y in the range, the share of the area's blocks where the flower is certain and the share where it's possible. It uses the map's sampling, so the list agrees with the layer at every height it names.

### The search

- **The constraint**, in [search.md](../specs/search.md)'s grammar:

  ```text
  constraint = ... | [ count ] "flower" BLOCK "field" INT [ "chance" PERCENT ] [ "at" "y" INT [ "to" INT ] ] place [ "step" INT ]
  ```

  For example: `require flower minecraft:cornflower field 200 at y 64 to 120 within 1000 of spawn`.

  - A field is a set of blocks at one height, connected through their sides, where the flower's chance is at least `chance` (100% by default, so the flower is certain). The field must have at least the given number of blocks.
  - With a range of heights, any one height in it will do, and the answer names the height.
  - `count` asks for that many separate fields.
  - The spec settles the details while this is built: the default step, how a sampled cell counts as `step²` blocks, and the score of a `prefer`.

- **Cost:**
  - The flower picked at a block doesn't depend on the seed. Only the biome does.
  - So the search first asks the biome sampler where the biomes whose features can grow the flower are, at the constraint's step (a cheap biome check, [search.md](../specs/search.md#biome-sampling)).
  - Then it samples the providers only there.
  - It works in a seed search and in a search within one seed (`find` with `here`) with no changes beyond the constraint itself.

### Front ends and the guide

- **CLI:**
  - `spyglass flower at X Y Z`: the answer at a block, and why: the biome, its features, and the provider's odds.
  - `spyglass flower heights X1 Z1 X2 Z2 FLOWER [--y A..B] [--step N]`: the best heights for an area.
  - `spyglass render --flowers [--flower FLOWER] --y Y`.
  - The search constraint.

  Each takes `--json`, in [cli-json.md](../specs/cli-json.md)'s style.

- **Viewer:**
  - The flower layer under the Layers panel. It uses the map's own height control, so the user can move through heights.
  - The key of the flowers shown, where clicking a flower highlights it.
  - A **Best height** action for a selected area that lists the heights where the highlighted flower covers most of the area. Clicking a height moves the view to it.
  - Hovering shows the answer at the block under the cursor.
- **Guide:** a flower-farm page, a cornflower farm in a flower forest, that walks through finding the field, picking its height, checking a block with `spyglass flower at`, and searching for a seed with a big field near the spawn.

## Tasks

- [x] Write `docs/specs/flowers.md` from the 26.3 bytecode:
  - bone meal's walk and its two outcomes;
  - how a biome's bone meal features are gathered and picked;
  - each provider's codec (26.2 and 26.3 spellings) and `getState`, with its draws;
  - the odds;
  - the block the biome and the noise are sampled at.

  Check 26.2 and 26.4-snapshot-8 against it (`cargo xtask version-diff --code`).

- [x] Load the flower providers, the `block_state_provider` registry, the bone meal tag, and each biome's bone meal features, for 26.2, 26.3 and 26.4.
- [x] Port the providers' `getState` and their odds (`spyglass_feature::flowers`), with unit tests for each provider's odds summing to 1.
- [x] Oracle: a `flowers` command, with 26.2's and 26.3's adapters if the harness's game API needs them (26.4 builds on 26.3). For each golden seed it writes `oracle-out/<version>/flowers/<seed>.json`:
  - for fixed regions at several heights, the zoomed biome and its bone meal features;
  - for each bone meal feature's provider, at fixed positions across the build height, its state under several fixed `LegacyRandomSource` seeds.
- [x] `Session::biome_at_block` and `Session::flower_at`, checked against the oracle's files (`cargo xtask oracle-test`), with the providers' states compared draw for draw.
- [x] `Session::flower_map` and the flower layer in `render`, with its colour table, and the spec's Layers list updated.
- [x] `Session::flower_heights`, with a test that its shares match the map at each height.
- [ ] The search constraint: parsing, canonical form, the biome-first sampling, scores, and the outcome. [search.md](../specs/search.md) gets its grammar and meaning.
- [x] CLI: `flower at`, `flower heights`, `render --flowers`, with `--json`; [cli-json.md](../specs/cli-json.md) updated.
- [ ] Viewer: the layer, the key and highlight, the hover answer, and Best height.
- [ ] Benchmarks: a flower map tile and a flower search added to `cargo xtask bench`, with their times in the spec.
- [ ] User guide: the flower-farm page, and the layer, command and constraint in [maps.md](../guide/src/maps.md), [command.md](../guide/src/command.md) and [seed-search.md](../guide/src/seed-search.md).
- [ ] Update the milestone's exit criteria.

## Verification

- **Oracle:** the oracle's `flowers` files match spyglass's answers at every sampled block and for every provider and random seed, for 26.2, 26.3 and 26.4-snapshot-8 (`cargo xtask oracle-test`). The test fails if any vanilla biome with a bone meal feature, or any bone meal feature, doesn't appear in the golden seeds' samples.
- **Odds:** for each provider, the odds match the share of states its `getState` draws over many random seeds, within sampling error. This is a unit test, since the odds are derived, not the game's.
- **Map and best heights:** the map agrees with `flower_at` at every pixel, a view equals its tiles, and `flower_heights` agrees with the map at the heights it names.
- **Search:** a search within one seed finds a field the map shows, at the height it names, and a seed search's top seed's field is checked the same way.
- **By eye:** the viewer's flower layer in a flower forest shows bands that move as the height changes. Checked with the `run-viewer` skill.
- `cargo xtask check` passes.

## Durable outputs

- [ ] `docs/specs/flowers.md`: bone meal on grass, the bone meal features, the providers and their odds.
- [ ] [render.md](../specs/render.md): the flower layer, its colours and highlight.
- [ ] [search.md](../specs/search.md): the flower-field constraint.
- [ ] [cli-json.md](../specs/cli-json.md): `flower at`, `flower heights`.
- [ ] [oracle.md](../specs/oracle.md): the `flowers` command and its files.
- [ ] An ADR if answers with odds need a new form in the API. Today's answers are certain or candidates ([ADR 0056](../adr/0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)), and a chance per flower is new. Decide when `FlowerAt` is designed.
- [ ] The user guide's flower-farm page.

## Notes

Newest first.

- 2026-11-15: `spyglass flower at` and `flower heights`, with their JSON, and the guide's flower-farms page, which `render --flowers` (committed before it) needed too. The page leaves out the viewer and search until they're built.

- 2026-11-15: The flower map, layer and best heights. A pixel that's down to chance shows its most likely flower, fainter the less likely, rather than a checker of its flowers: plains' nine flowers made a checker noise, where the likeliest one at its chance reads at a glance. `render --flowers` and `--flower <id>` came with it, as the layer needed `Layers` fields every front end sets. A view still equals its tiles with flowers on.

- 2026-11-15: `Session::biome_at_block` and `Session::flower_at`, through the API, match the oracle's block biomes in all three versions. The zoom needs only `BiomeZoom` and the session's biomes, not the block settings, so a flower answer doesn't load the chunk generator. [ADR 0075](../adr/0075-flower-answers-carry-exact-chances.md) (proposed) records the answers' exact chances.

- 2026-11-15: `oracle flowers` and its comparison: all 25,600 provider draws (state and next draw) and about 435,000 block biomes match in 26.2, 26.3 and 26.4-snapshot-8. The harness needs three adapter calls (each biome's bone meal features, a provider's state, the zoomed biome), and one 26.3 implementation compiles under 26.4. It's part of `golden` and CI, as kind `flowers`, at an estimated minute.

- 2026-11-15: The spec, loading and the providers' port are done, with tests passing on 26.2, 26.3 and 26.4-snapshot-8 (the odds against 20,000 draws per feature and position):
  - 26.2's providers work in `double`, 26.3's in `float`, since 26.3's noise returns a `float`. The noise's form in the data decides which, as for the geode.
  - 26.4-snapshot-8 changes nothing that matters: `Math.clamp` for `Mth.clamp`, and a list built another way.
  - `getBoneMealFeatures` walks each placed feature's feature, then what the selectors nest (`random_selector`'s features then `default`, `random_boolean_selector`'s true then false, `simple_random_selector`, `sequence`, `overlay`, `weighted_random_selector`), depth first, keeping repeats.

- 2026-11-15: The 26.3 bytecode shows four things the milestone didn't say:
  - The noise providers are seeded by their own `seed` field, not the world's, so a field's shape is the same in every world, and only the biome depends on the seed. The search uses that.
  - Plains-type biomes (`noise_threshold`) only split tulips from the rest; which flower grows is random within each. So a plains "field" of one flower is only ever odds: the constraint's `chance` exists for that.
  - Bone meal looks up the biome at the flower's own block, with `BiomeManager`'s zoom, not at the surface or the quart. The public `biome_at` gives the quart's biome, so the API needs the zoomed one.
  - Deep dark and dripstone caves have the plains provider, so underground farms there are fields too.
