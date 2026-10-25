# m02. Overworld biome lookup

- Status: Draft
- Milestone: [02. RNG, noise and biomes](../roadmap/02-rng-noise-biomes.md)
- Started: —
- Completed: —

## Goal

Overworld biomes at quart (1:4) resolution for any seed, matching the oracle's golden files: exact where the minimum is unique, a member of the tied set where it isn't. Covers the milestone's first exit criterion and the biome-lookup part of the third. Completing this plan completes the milestone.

The last of three plans for milestone 02. Needs [m02-rng-noise](completed/m02-rng-noise.md) and [m02-density](m02-density.md).

## Facts established up front

Checked against `bundles/26.3/`, `oracle-out/26.3/` and the 26.3 jar (`javap`) on 2026-10-25.

- **Which settings.** `worldgen/world_preset/normal.json` gives the overworld a `minecraft:noise` generator with `biome_source: {"type": "minecraft:multi_noise", "preset": "minecraft:overworld"}` and `settings: "minecraft:overworld"`. `multi_noise_biome_source_parameter_list/overworld.json` only names the preset, so the parameters come from the data-generator report.
- **The parameter list.** `reports/biome_parameters/minecraft/overworld.json` is `{"biomes": [...]}` with 7,594 entries over 56 biomes. Each is `{"biome": "minecraft:mushroom_fields", "parameters": {"continentalness": [-1.2, -1.05], "depth": 0.0, "erosion": [-1.0, 1.0], "humidity": [-1.0, 1.0], "offset": 0.0, "temperature": [-1.0, 1.0], "weirdness": [-1.0, 1.0]}}`: a `[min, max]` pair or a single number per parameter. Values are recovered as quantized longs by round-to-nearest, never truncation ([data-sources.md](../specs/data-sources.md#climate-parameter-values)).
- **Router names vs parameter names.** The router's `vegetation` is the parameter list's `humidity`, and `ridges` is `weirdness`. `Climate$Sampler`'s fields are `temperature`, `humidity`, `continentalness`, `erosion`, `depth`, `weirdness`, in that order, as are `Climate$TargetPoint`'s longs.
- **Selection** (`net.minecraft.world.level.biome`): `Climate$ParameterPoint.fitness(TargetPoint)` is a `long`: the sum of `Mth.square(Climate$Parameter.distance(long))` over the six parameters, plus `Mth.square(offset)`. `Climate$ParameterList` has `findValue`, `findValueBruteForce` and `findValueIndex`, the last through `Climate$RTree` (`CHILDREN_PER_NODE = 19`, `build`, `bucketize`, `cost`, `search`, and a thread-local `lastResult` that warm-starts each search). `MultiNoiseBiomeSource.getNoiseBiome(TargetPoint)` is the point path; `createResolverForChunk` is the chunk path the server uses.
- **Ties.** The tree only replaces its warm-start result with something strictly closer, so tied cells depend on lookup history ([oracle.md](../specs/oracle.md#comparing-biomes-exact-except-ties)). The oracle's own exact tie search (`oracle/src/spyglass/oracle/TieSearch.java`, our code) finds every point at the minimum with a bounding-box tree and was checked against brute force.
- **The golden set** is in `oracle-out/26.3/biomes/overworld/`: 10 seeds × 5 regions, 50 files, 63.9 million cells, of which 52,297 are tied. The origin region is 256 × 256 quarts × 96 levels; the far and border regions are 16 × 16 × 96. The header carries the numeric seed, so the text seed `spyglass` needs no hashing on the Rust side. `spyglass-oracle` reads the files (`BiomeGrid::read`, `Header::accepts`, `biome_files`).
- **Cost.** Five of the six climate functions don't depend on y (`y_scale` 0, no y inputs); only `depth` does, through a y `gradient` added to the 2D `offset`. A full-height column needs the 2D values once and `depth` 96 times, so evaluating by column is about two orders of magnitude cheaper than per cell, as the game's own `SLICE_UNIFORM_AXES` rewrite and `cache` exploit.

## Approach

1. **Parameter list** in `spyglass-biome`: parse the report into `ParameterPoint`s (six `[min, max]` long pairs plus `offset`) in list order, with biome ids.
2. **Nearest search with ties.** An exact search for the minimum fitness that returns every parameter point at that minimum, deduplicated by biome, so a cell is reported as `Unique(biome)` or `Tied(set)`. The default answer for a tied cell is the first in list order, which is what `findValueBruteForce` returns, so results are deterministic and thread-independent. Brute force over all 7,594 points is the reference implementation and the test oracle for the fast search. The fast search is a bounding-box tree like the oracle's `TieSearch`, pruning only strictly farther boxes.
3. **Climate sampler**: bind the router's six climate functions for a seed ([m02-density](m02-density.md)), sample at quart → block coordinates, quantize with `(long)(f * 10000.0f)`, and search.
4. **API** (`spyglass-biome`, until the `spyglass` crate exists in milestone 03): load the overworld from a bundle and a seed; `climate_at(q)`, `biome_at(q)` (with tie information) and a region fill (`biomes_in(min, size)`) that evaluates per column. Column evaluation is only allowed where the graph proves a subtree independent of y, so it can't change a value.
5. **Golden comparison**: an ignored integration test that, for every golden file, fills the region and checks each cell against the file by the tie rule. Seeds run on separate threads (`std::thread::scope`; no new dependency). It reports the first mismatches with seed, cell, climate target and both answers, rather than only a count.

## Tasks

- [ ] `spyglass-biome`: parameter-list loading from the bundle report, with round-to-nearest quantization. Unit tests on hand-written report snippets, including values the game's truncation would get wrong.
- [ ] `spyglass-biome`: `fitness` and brute-force search returning the minimum and the full tie set, deduplicated by biome.
- [ ] `spyglass-biome`: fast exact search (bounding-box tree), checked against brute force on random targets and on every probed climate target. Port the algorithm from our `TieSearch.java`, not from `Climate$RTree`'s warm-start search.
- [ ] Decide and spec the answer for a tied cell (first in list order is proposed above) and how ties are exposed in the API.
- [ ] `spyglass-biome`: climate sampler over the bound router and `biome_at` / `climate_at`.
- [ ] `spyglass-biome`: region fill with per-column evaluation of y-independent subtrees, checked against per-cell evaluation on random regions.
- [ ] Ignored golden test (`crates/spyglass-biome/tests/golden.rs`): every cell of every golden file passes for 26.3, using `bundles/<version>/` and `oracle-out/<version>/`. Runs with `cargo test --release -p spyglass-biome -- --ignored`; target a few minutes or less on a few cores.
- [ ] Spot-check that `biome_at` agrees with the region fill on the golden cells (both paths in the API give the same answers).
- [ ] `docs/specs/biome-lookup.md`: parameter loading, the climate sampler (router → parameter mapping, quart → block, quantization), fitness, the tie rule and the tie answer, and the region fill's column rule.
- [ ] `docs/specs/oracle.md`: link the Rust-side comparison and how to run it.
- [ ] `docs/getting-started.md`: running the golden comparison (coordinate with whoever is editing it).
- [ ] Milestone wrap-up: tick the exit criteria in the milestone, move the m02 plans and the milestone to `completed/`, update the roadmap and plan indexes, fix links.

## Verification

- Unit tests: quantization, fitness and brute-force search on hand-written data; fast search equals brute force on random targets.
- The ignored golden test passes on all 50 files of the 26.3 golden set: zero cells outside the rule, including negative seeds, `i64` extremes, negative coordinates, ±1,000,000 and the world border. It also reports how many tied cells it saw, which should equal the golden files' 52,297.
- Running it twice gives the same result (no thread-dependent answers).

## Durable outputs

- [ ] `docs/specs/biome-lookup.md`
- [ ] `docs/specs/oracle.md`: Rust-side comparison
- [ ] `docs/specs/README.md`: list the new spec
- [ ] `docs/architecture.md`: the Biomes row, if the search or the API shape differs from "nearest-biome search tree"
- [ ] `docs/getting-started.md`: running the comparison

## Open questions

- **Which tied biome to return.** First in list order matches the game's brute-force path and is deterministic, but the game in practice returns whatever its warm-started tree found. Any member passes the oracle; the choice only affects what users see and should be stable.
- **Large Biomes and Amplified.** Out of scope: the milestone is the Default preset. Both reuse the same code with different settings and router files (`overworld_large_biomes/*`), so they should cost little later.

## Notes

- 2026-10-25: Plan drafted.
