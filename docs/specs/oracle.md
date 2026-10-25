# Differential oracle

Status: draft. Biome grids are specified; structure starts and heightmaps come later.

Correctness is tested against the real game, not assumed.

## Design

A standalone Java harness in `oracle/`, compiled against the user's own server jar, calls the game's own functions directly, following the same code paths the server uses. This is much faster than `/locate`. The approach is decided in [ADR 0004](../adr/0004-oracle-standalone-harness.md). It dumps golden files:

- biome grids (below);
- structure starts and bounds (later);
- heightmaps (later).

CI runs the Rust crates against these for a fixed set of seeds per supported version. Golden files are generated locally into `/oracle-out` (gitignored), since they are derived from the game. No game-derived values are committed, not even small test sets ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)).

## Running it

`cargo xtask oracle` builds and runs the harness. Every subcommand takes `--game-version` (default `latest`), `--cache-dir` and `--java`.

| Command                                                         | Does                                                                                       |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `build`                                                         | Fetch and verify the server jar, unpack its libraries, compile the harness.                |
| `biomes <seed> [--dimension] [--x] [--z] [--size] --out <file>` | Write one golden biome file. Block coordinates; x, z and size must be multiples of 16.     |
| `check-save <world> [--dimension]`                              | Compare the harness with the biomes stored in a real save. Fails on any unexplained cell.  |
| `probe`                                                         | Write the [probe files](#probe-files) into `oracle-out/<version>/probe/`.                  |
| `golden`                                                        | Regenerate the whole golden set below, probe files included, into `oracle-out/<version>/`. |

`build` runs automatically before the others, and does nothing when nothing changed:

1. The server jar comes from the per-user jar cache shared with the extractor ([data-sources.md](data-sources.md#jar-cache)), verified by SHA-1.
2. The bundler is unpacked into `target/oracle/<version>/classpath/`, once per jar SHA-1.
3. A JDK of the version JSON's `javaVersion.majorVersion` is found with the usual [Java discovery](data-sources.md#java-runtime), filtered to JDKs.
4. `oracle/src/**/*.java` is compiled with `javac --release <major> -Xlint:all -Werror` into `target/oracle/<version>/classes/`, unless the sources, jar and JDK are unchanged.

The harness runs with `target/oracle/<version>/run/` as its working directory, where the game writes `logs/`, and with `oracle/log4j2.xml` so only the game's warnings reach the console.

### What the harness does

- **Game state.** `SharedConstants.tryDetectVersion`, `Bootstrap.bootStrap`, then the `WorldLoader` sequence with only the vanilla pack: static registries, tags, then the worldgen registries into the `WORLD` layer. Dimensions come from `WorldPresets.createNormalWorldDimensions`, which is what a new Default world gets. A seed becomes a `RandomState` through `RandomState.create`, as `ChunkMap` does. Text seeds are converted by `WorldOptions.parseSeed`, as the world-creation screen does.
- **Biomes.** Per chunk, as `ChunkGenerator.doCreateBiomes` does: a cached `SamplerContext` on a pooled buffer arena, `createClimateSampler`, then `BiomeSource.createResolverForChunk` over the full height of the dimension type.
- **Ties.** For a multi-noise biome source, the harness also runs a copy of `MultiNoiseBiomeSource.createResolverForChunk` that keeps each cell's climate target, and an exact search for every parameter point at the minimum fitness. Every cell is checked: the game's resolver and the game's `getNoiseBiome(target)` must both return a member of that set. Every tied cell, and one cell in 1,000, is also checked against a brute-force search using the game's own `Climate.ParameterPoint.fitness`. Any failure stops the run. It means the copy or the search has drifted from the game.

## Comparing biomes: exact, except ties

The game picks a biome by finding the parameter point with the minimum `Climate.ParameterPoint.fitness`, an integer distance. When two or more points tie at the minimum, the answer depends on lookup history: `Climate.RTree` starts from each thread's previous result and only replaces it with something strictly closer. Chunks generate on worker threads in no fixed order, so **the game can give a tied cell different biomes in different runs.** Measured in 26.3, about 1 cell in 1,000 is tied.

So for biomes:

- Golden data records, for each cell, the biome when the minimum is unique, or **the full set of tied biomes** when it isn't. Points that tie but map to the same biome aren't a tie.
- spyglass passes a cell if it matches exactly where the minimum is unique, or returns any member of the tied set where it isn't.
- spyglass should be able to report a cell as tied, so users can see where the game itself is unpredictable.

### Rust-side comparison

`spyglass-biome/tests/golden.rs` checks every cell of every overworld golden file against `BiomeSource::with_seed(seed).biomes_in(min, size)` by this rule. It also requires spyglass's tie set to equal the file's exactly, and its tied-cell count to match the header. It runs one thread per seed and reports the first mismatches per file, with the cell, its climate target and both answers. Run it with the probe and region tests:

```sh
cargo test --release -p spyglass-biome -- --ignored
```

It needs the golden set and a bundle for the same version (`bundles/<version>/`). For 26.3 it checks 63.9 million cells, 52,297 of them tied, in about 15 seconds (one thread per seed, on a 16-core machine). What spyglass answers for a tied cell is in [biome-lookup.md](biome-lookup.md#ties).

## Golden biome files

One file per (seed, dimension, region). All integers are little-endian.

| Part      | Contents                                                                                                             |
| --------- | -------------------------------------------------------------------------------------------------------------------- |
| Magic     | 8 bytes: ASCII `SPYBIOME`.                                                                                           |
| Header    | `u32` length, then that many bytes of UTF-8 JSON, below.                                                             |
| Cells     | `size[0] × size[1] × size[2]` `u16` palette indices. x varies fastest, then z, then y.                               |
| Tie table | `ties` entries, by ascending cell index: `u32` cell index, `u16` count _n_ ≥ 2, _n_ ascending `u16` palette indices. |

The file ends exactly after the tie table. A tied cell's entry in the cells array is its tie set's lowest palette index, so the file doesn't depend on which tied biome the game happened to pick, and is byte-identical across runs.

The header is compact JSON with these keys, in this order:

| Key           | Meaning                                                                       |
| ------------- | ----------------------------------------------------------------------------- |
| `format`      | Format version, currently `1`. Readers reject other values.                   |
| `harness`     | Harness version. Bumped when the harness's output for a game version changes. |
| `version`     | Game version id, e.g. `"26.3"`.                                               |
| `dataVersion` | The game's data version, e.g. `5023`.                                         |
| `seed`        | The world seed as a signed 64-bit integer (after text-seed hashing).          |
| `dimension`   | e.g. `"minecraft:overworld"`.                                                 |
| `min`         | Minimum quart x, y, z.                                                        |
| `size`        | Size in quarts along x, y, z.                                                 |
| `palette`     | Biome ids present in the file, sorted, unique.                                |
| `ties`        | Number of tie-table entries.                                                  |

Regions are chunk-aligned and full height: `min[1]` and `size[1]` are the dimension type's `minY` and `height` in quarts (−16 and 96 for the overworld).

Files are laid out as `oracle-out/<version>/biomes/<dimension path>/<seed>/<region>.bin`, for example `oracle-out/26.3/biomes/overworld/-1/origin.bin`. The seed folder is the seed as given to the harness, so `spyglass` for the text seed. The `spyglass-oracle` crate reads them; it's test support, used only as a dev-dependency.

### Golden set

The overworld, for these seeds:

| Seed                                           | Why                                                           |
| ---------------------------------------------- | ------------------------------------------------------------- |
| `0`, `1`, `-1`                                 | Trivial and negative seeds.                                   |
| `-9223372036854775808`, `9223372036854775807`  | `i64` extremes.                                               |
| `spyglass`                                     | A text seed, hashed by the game (`String.hashCode`, widened). |
| `-4893098694207956070`, `2514887864703934023`  | The two real 26.3 worlds the harness was validated against.   |
| `-8228862705229265597`, `-1918002950002395028` | Fixed random seeds.                                           |

And these regions per seed, all full height:

| Region       | Blocks (x and z)           | Why                                                   |
| ------------ | -------------------------- | ----------------------------------------------------- |
| `origin`     | −512 to 511                | Bulk coverage, both signs of x and z.                 |
| `far-pos`    | 1,000,000 to 1,000,063     | Large coordinates.                                    |
| `far-neg`    | −1,000,064 to −1,000,001   | Large negative coordinates.                           |
| `border-pos` | 29,999,920 to 29,999,983   | Just inside the world border: overflow and precision. |
| `border-neg` | −29,999,984 to −29,999,921 | The same, negative.                                   |

That's about 6.4 million cells per seed. The origin region is 1,024 blocks square, not larger, to keep the Rust side's full comparison quick.

## Probe files

Golden files hold biomes only. Porting the layers underneath (random sources, noise) needs the game's raw values, so `cargo xtask oracle probe` writes them into `oracle-out/<version>/probe/`, and `golden` regenerates them too. Like golden files, they're generated locally and never committed ([ADR 0008](../adr/0008-game-derived-test-values-stay-local.md)). The Rust tests that read them are `#[ignore]`d.

Each file is UTF-8 JSON:

```json
{
  "format": 1,
  "harness": 1,
  "version": "26.3",
  "data_version": 5023,
  "cases": [
    {
      "test": "next_int_bounded",
      "kind": "xoroshiro",
      "seed": -1,
      "bound": 7,
      "type": "i64",
      "values": [3, 0, 6]
    }
  ]
}
```

A case names what was called (`test`, and `method` where a test has several), carries every parameter needed to reproduce it, and lists the results. The Rust side replays the cases it finds, so the seed and point lists live only in the harness. `type` says how to read `values`:

| `type` | Values                                                                                  |
| ------ | --------------------------------------------------------------------------------------- |
| `i64`  | JSON integers. `int` results are widened; booleans are 0 or 1.                          |
| `f32`  | Hex strings of the raw bits, 8 digits (`Float.floatToRawIntBits`).                      |
| `f64`  | Hex strings of the raw bits, 16 digits (`Double.doubleToRawLongBits`).                  |
| `text` | One string, e.g. the game's own `parityConfigString` description of a noise or factory. |

Floating-point inputs (`point`) are hex `f64` bits too, so nothing depends on decimal parsing. Parameters: `seed` (a `long`), `kind` (`xoroshiro` for `XoroshiroRandomSource`, `legacy` for `LegacyRandomSource`, each from `new ...(seed)`), `bound`, `pos` (block `[x, y, z]`), `name` (a string hashed by `seedFromHashOf`, or a noise id).

**`rng.json`** covers `RandomSupport` (`mix_stafford13`, `upgrade_seed_to_128bit`, `upgrade_seed_to_128bit_unmixed`, `seed_from_hash_of`), and for each kind and seed a fresh source per sequence: `next_long`, `next_int`, `next_int_bounded`, `next_float`, `next_double`, `next_gaussian`, `next_boolean` (8 values each), `gaussian_interleaved` (Gaussian, long, Gaussian, Gaussian, `nextInt(10)`, with Gaussians as raw `f64` bits), `consume_count_5` (two longs after `consumeCount(5)`), `fork` (two longs from the fork, then two from the parent), and from `forkPositional()`: `positional_parity`, `positional_at` and `positional_from_hash_of` (two longs each).

**`noise.json`** has, for each golden seed: `perlin` (a `PerlinNoise` built from `new XoroshiroRandomSource(seed)`) and `perlin_parity`; and for each overworld climate noise (`temperature`, `vegetation`, `continentalness`, `erosion`, `ridge`, `offset`) as `RandomState` creates it, `normal_noise` and `normal_noise_parity`. Samples have `method` `get_3d` or `get_2d`, one point each.

**`density.json`** has, for each golden seed, using the overworld's noise settings:

- `density`: each named climate function (`name`, e.g. `minecraft:overworld/offset`) at block `pos`, via `RandomState.sampleBlockValueUncached`;
- `climate`, at quart `pos`, from `RandomState.createClimateSampler` as chunk generation builds it: `method` `floats` (the six values, `f32`) and `target` (the six quantized longs of `Climate.Sampler.sample`);
- for each of four chunks (`pos` is `[chunk x, 0, chunk z]`, full overworld height): `climate_volume_vs_point`, the number of cells where the game's volume path and point path differ for any of the six functions, and `climate_digest`, every climate float in the chunk hashed as `h = h·1000003 + bits` (wrapping `long`, `bits` the float's raw bits as unsigned, starting from `h = 1`), over quart cells with y outermost, then z, then x, and the six values in order.

## Independent check: real world saves

Every chunk in a save that has reached the biomes generation step stores the biomes the game generated, at quart resolution. Comparing the harness against saves made in the target version, with the Default preset and no worldgen datapacks, checks the harness independently of itself. Ties are allowed, as above.

`check-save` reads the seed from `data/minecraft/world_gen_settings.dat` (`data.seed`, where 26.x keeps it), and the chunks from `dimensions/<namespace>/<path>/region/` with the game's own `RegionFile` and `NbtIo`. It compares every stored cell of every chunk whose status is `biomes` or later.

| World seed             | Chunks | Cells compared | Tied cells | Save chose another tied biome | Unexplained |
| ---------------------- | -----: | -------------: | ---------: | ----------------------------: | ----------: |
| `-4893098694207956070` | 29,416 |     45,182,976 |     39,983 |                        17,679 |           0 |
| `2514887864703934023`  |  8,199 |     12,593,664 |     11,587 |                         4,608 |           0 |

## CI

CI isn't set up yet. When it is:

- Install a JDK of the target major (Temurin through `actions/setup-java`).
- Let `cargo xtask oracle golden` download the server jar from Mojang during the run. Nothing derived from the game is committed or stored as an artifact that outlives the run.
- Cache the jar cache directory (`--cache-dir`) keyed by the jar's SHA-1 from the version JSON, so the 60 MB download happens once per version.
- Generating the golden set takes about a minute on a few cores. Golden files could be cached the same way, keyed by jar SHA-1 plus a hash of `oracle/`.

## Open questions

- Nether and End golden data. The harness already handles any dimension; the golden set adds them when milestone work needs them.
