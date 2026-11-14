# m28. Fixes from the 2026-11-13 review

- Status: In progress
- Milestone: [28. Fixes from the 2026-11-13 review](../roadmap/28-review-fixes.md)
- Started: 2026-11-13
- Completed: YYYY-MM-DD

## Goal

Fix or close every finding of the review of 2026-11-13, which covered the branch carrying milestones 23 to 26. This plan covers all of milestone 28's exit criteria.

`cargo xtask check` passed at the time of the review. The review found:

- five parity issues
- eight bugs, one of them a memory-exhaustion path through data packs
- functions and files that grew too long
- documentation drift
- suggestions for concurrency, matching, performance, tests and CI

## Findings

Findings marked _(confirm)_ depend on how the game behaves, which the review read from code but didn't test. The first task for each is a test or a game comparison that shows the failure. If nothing fails, the finding is closed in the notes and the test is kept. Line numbers are as of commit 10a05c8.

- **Parity**
  - P1. `pow` rounds exact midpoints wrongly (`spyglass-math/src/explog.rs:161`).
    - Ziv's rounding test can't settle a value that lies exactly halfway between two doubles, so `value.hi` is returned, and it's often the wrong neighbour.
    - `pow(134217727.0, 2.0)` gives `18014398241046530` where Java gives `…528`. 25% of odd 27-bit `m` squared and 19% of 18-bit cubes differ.
    - Worldgen doesn't reach this: its exponents aren't integers, and `float` bases round back the same. It does break the crate's claim.
  - P2. The 2⁻⁹⁵ bound in trig's rounding test doesn't hold near multiples of π/2 when `k` is large (`trig.rs:57-61`).
    - `k * PI_2[2]` is a plainly rounded product, and π/2 is cut off after three parts.
    - For k in [2·10⁵, 6.7·10⁵], |r| gets down to about 2⁻⁵⁴·³, so the relative error is near 2⁻⁹⁰.
    - The oracle's edge set stops at k = 20,000.
  - P3. _(confirm)_ Chunks saved by 1.18.0 and 1.18.1 are answered as "certainly none" (`spyglass/src/saved.rs:243`, `spyglass-world/src/chunk.rs:107`).
    - These chunks keep the current `structures.starts` layout but use the old start keys (`minecraft:village`, `ocean_ruin`, `ruined_portal`, `endcity`). 1.18.2's `StructuresBecomeConfiguredFix` renames them by biome.
    - spyglass drops ids it doesn't know, so the chunk reads as `Some([])`.
  - P4. _(confirm)_ A saved start must have `id`, `ChunkX` and `ChunkZ` to count (`chunk.rs:150-158`). The game's `loadStructures` counts any non-empty start whose `id` isn't `INVALID`.
  - P5. _(confirm)_ `pack.mcmeta` format numbers are read as strict `i32` (`spyglass-data/src/pack.rs:769`).
    - The game's `Codec.INT` takes `Number.intValue()`, so it accepts `121.0` and `4294967417`, which wraps to 121. spyglass refuses both.
    - The `parts.len() > 256` cut-off (`pack.rs:726`) is undocumented.
  - P6. _(confirm)_ One entry with `\`, `:`, a leading `/` or an empty segment marks the whole pack unreadable (`pack.rs:468-474`). The game skips only that file, because its `Identifier` fails.
  - P7. _(confirm)_ A malformed tag file is merged (`bundle.rs:300-305`).
    - "Malformed" here means an element that isn't an id, a `#tag` or `{id, required}`, or a `replace` that isn't a bool.
    - The game logs "Couldn't read tag list" and skips the file. spyglass merges it, and the world then fails to load.
- **Bugs**
  - B1. Pack templates inflate without a limit.
    - `spyglass-feature/src/place/template.rs:121`, `spyglass-structure/src/jigsaw.rs:1303` and `spyglass-feature/src/reach.rs:526` call `nbt::read_compressed`, which allows `u64::MAX` bytes.
    - A pack's `.nbt` file reaches these readers since milestone 26, so a 32 MiB gzip bomb can expand without bound.
  - B2. `pow` returns NaN for |y| above about 1.34e300 (`dd.rs:33`, `explog.rs:152`).
    - `split(y)` overflows to `inf − inf`, so `pow(2.0, 1e301)`, `pow(0.5, f64::MAX)` and `pow(2.0, -1e305)` give NaN. Java gives ∞ or 0.
    - The comment saying nothing reaches this range is wrong.
  - B3. `ChunkContents::has_features` leaves out `heightmaps` (`chunk.rs:95`). That status exists in 1.18 to 1.19, so containers in proto chunks saved at it lose their save state.
  - B4. `level.dat`'s enabled packs aren't deduplicated (`spyglass-world/src/packs.rs:50`, `spyglass/src/save.rs:611`). [data-packs.md](../specs/data-packs.md) says "A duplicate counts once, where it first appears".
  - B5. A saved start is printed as certain when locate keeps it uncertain.
    - The docs at `spyglass/src/lib.rs:984` and `world-saves.md:85` say a `from_save` start "is certain".
    - Locate's `certain && exact` (`spyglass-structure/src/locate.rs:421`) keeps it uncertain in a concentric ring's tied or platform-dependent alternative chunk.
    - The CLI prints "saved" there (`spyglass-cli/src/locate.rs:80`).
  - B6. Two viewer tooltips contain runs of 14 to 18 spaces mid-sentence (`spyglass-viewer/src/app/side_panel.rs:499,507`), from missing `\` continuations.
  - B7. A rejected oracle run leaves a stray variant behind.
    - `build()` writes `oracle-out/<v>+<label>/packs.json` (`xtask/src/oracle.rs:906`) before `test-pack` rejects `--pack`/`--name` or `locate` rejects `--pack` with `--world`.
    - `oracle-test` then tests the stray variant.
  - B8. `Save::inspect` fails outright on a bad bundle (`Bundle::open(...)?` in `save.rs`). It used to report one as a warning.
- **Length and organisation**
  - S1. `spyglass-viewer/src/app/map.rs` is 1,628 lines, and `App::map` is about 730 of them. Other long functions in the viewer:

    | Function           | File            | Lines (approx.) |
    | ------------------ | --------------- | --------------- |
    | `map_controls`     | `map.rs`        | 146             |
    | `describe`         | `map.rs`        | 127             |
    | `container_window` | `map.rs`        | 109             |
    | `map_legend`       | `map.rs`        | 105             |
    | `layers_tab`       | `side_panel.rs` | 121             |
    | `structure_list`   | `side_panel.rs` | 136             |

  - S2. `spyglass/src/render.rs:852` `map_features` is about 127 lines. Its generated and slime-chunk blocks repeat the same min/max and scale-check code.
  - S3. `spyglass-data/src/pack.rs` is about 850 non-test lines, and `FormatFields::validate` (`:625-716`) is about 92.
  - S4. Long xtask modules:
    - `xtask/src/oracle.rs` is 1,119 lines, with `run()` about 140.
    - `xtask/src/locate.rs` is 932 lines, with `run()` about 127 and `run_map_cases()` about 105.
    - `xtask/src/code_diff.rs` is 918 lines, with `code_map()` about 96.
    - `xtask/src/math_report.rs:158` `report` is about 100 lines.
  - S5. `inverse_factorials` and `rounded` live in `trig.rs` (`:41,139`), but `explog.rs` uses them too.
- **Documentation**
  - D1. `spyglass-math/src/lib.rs:9-10` says no measured argument was left open, but [java-math.md](../specs/java-math.md) reports 9 of 6.2 million.
  - D2. `xtask/src/math_report.rs:33` describes the platform's libm as "what spyglass computes today".
  - D3. The workspace `Cargo.toml:69` calls `libm` only a math-report candidate, but `spyglass-math` also uses it at runtime for |x| ≥ 2²⁰.
  - D4. `trig.rs:155` says "past the midpoint below 1". 1 − 2⁻⁵³ is exactly the double below 1.
  - D5. `oracle/src/spyglass/oracle/Main.java:42` says `--pack` is a folder, but zips work too.
  - D6. A stale doc line ("Slime chunks as the library's features…") sits above `chunk_features` (`spyglass-viewer/src/tiles.rs:601`).
  - D7. `SaveChunks`' doc comment has an odd line break (`saved.rs:19-21`).
  - D8. `xtask/src/citations.rs:61` doesn't say that `split_once("//")` also matches inside string literals.
  - D9. `Containers.java:476` matches the message prefix `"Requested out of range value"` without naming the game method that throws it.
  - D10. The comment at `spyglass-blocks/src/lib.rs:448-477` implies that cloning the random guards an observable effect. No ported carver calls `next_gaussian`.
- **Suggestions**
  - G1. The `SaveChunks` cache (`saved.rs:16,158`, and `entity_containers`):
    - It holds its mutex while it reads and decompresses a chunk, so parallel queries read one chunk at a time.
    - It `expect`s a lock that isn't poisoned.
    - It grows without limit and keeps every block entity's `components` tree.
  - G2. _(confirm)_ A block-entity container is matched by position only (`saved.rs:215`), so a chest a player placed in that spot reads as "Filled".
  - G3. _(confirm)_ Proto chunks keep their entities in the chunk's own `entities` list (`saved.rs:202`), so minecarts in partly generated chunks are always "unknown".
  - G4. `sin_table` (`spyglass-density/src/volume.rs:1063`) calls `spyglass_math::sin` on every lookup, about 50 to 100 times slower than before. The cosine mode that uses it can now be enabled by data packs.
  - G5. `sha1_hex(...).unwrap_or_default()` (`pack.rs:241,246,249`) would give an empty hash on failure, and two packs with that empty hash would share a cache identity.
  - G6. Test gaps:
    - `correctly_rounded.rs` has no NaN, ±∞ or midpoint inputs.
    - No test covers `sin` or `cos` of NaN or ∞.
    - Its `_ =>` arm would run `pow` for a mistyped function name.
    - Its assert message has raw newlines in string literals.
    - The pack fuzz target stops before `Bundle::with_packs`, `look_up`, `merged_tag` and `registry_ids`.
  - G7. CI:
    - `math.yml` doesn't run when `Cargo.lock` or `rust-toolchain.toml` changes.
    - `release.yml:92-109` still interpolates `${{ matrix.* }}` in `run:`. The values are static, but `ci.yml` has already moved to `env:`.
  - G8. Logic repeated in xtask:
    - `split_once('+')` for the base version appears four times, beside `spyglass_oracle::base_version`.
    - `"packs.json"` is written out twice.
    - The no-carvers pack path (`oracle.rs:~545`) has no variant in it, so two concurrent runs delete each other's pack.

## Approach

The work is in seven phases. Each phase is one or more commits that pass `cargo xtask check` on their own, and `cargo xtask oracle-test` where they touch worldgen. Small fixes come first, then the input and parity work, and refactors come last, so that they move code that is already fixed. Every bug and parity fix lands with a test that fails before it. This is the pattern m10 followed.

1. **Quick fixes and docs** (B1, B3, B4, B6, D1–D10, G5).
   - **B1:** one shared cap for templates. The templates come from the bundle or from a pack's `pack::MAX_FILE` read, and are capped through `nbt::read_compressed_limited`. The three call sites use it, and over-limit is an error, not a skip.
   - **Retire the unlimited reader:** consider making the unlimited `read_compressed` `#[cfg(test)]`, or removing it, so that no new caller can reach an unlimited read.
   - **B6:** after the fix, grep every non-raw literal with a raw newline, the way m10's R10 did. If more turn up, add a hook or test so they can't come back.
2. **`spyglass-math`** (P1, P2, B2, S5, G6's math part, G4). Governed by [ADR 0070](../adr/0070-java-math-correctly-rounded.md) and [java-math.md](../specs/java-math.md).
   - **P1:** when `y` is an integer and `x`'s significand times `|y|` fits in 106 bits, compute `x^y` exactly in double-double and round half-even.
     - Otherwise, when Ziv's test fails and the double-double value is exactly a midpoint, round half-even.
     - `y == 2.0` is `x * x`, as HotSpot's C2 emits.
     - Add midpoint cases to `JavaMath.java`'s `integerPowers` set.
   - **P2:** compute `k * PI_2[2]` with `two_prod`, and add a fourth part of π/2. Alternatively, widen the bound by |x|/|r|. Extend `trigEdges` to k = 2²⁰.
   - **B2:** in `pow_positive`, return ∞ or 0 for |y| ≥ 2⁶⁴ before splitting, chosen by whether `x > 1` matches `y > 0`, with tests. Fix the comment at `dd.rs:33`.
   - **S5:** move `inverse_factorials` and `rounded` into `dd.rs`, or into a `series` module.
   - **G4:** build the 65,536-entry table once in a `OnceLock`, shared with `carver.rs`'s table if the two are the same `Mth.SIN`. Time `sin_table` in a microbenchmark.
   - **Re-measure:** rerun `cargo xtask oracle math` and `cargo xtask math-report` on Windows, Linux and macOS. The open count in java-math.md must not grow, and no case may come out wrong.
3. **Saves** (P3, P4, B5, B8, G1–G3). Governed by [ADR 0071](../adr/0071-answers-from-a-save.md) and [world-saves.md](../specs/world-saves.md).
   - **P3:**
     - Confirm with a world made in 1.18.1, or from `StructuresBecomeConfiguredFix`'s data version in the 1.18.2 jar.
     - Then treat chunks below that data version as `SavedStarts::Legacy`, predicted rather than certain.
   - **P4:**
     - Confirm from `loadStructures`' bytecode in each supported version.
     - Then base presence on a non-empty compound whose `id` isn't `INVALID`, with the chunk position optional.
   - **B5:** make the CLI and API print "saved" only when the start is also certain. Correct `lib.rs:984` and world-saves.md to say a saved start is certain except in a ring's alternative chunk. Making ring alternatives exact would need a proof that the game visits that chunk, and that is out of scope.
   - **B8:** turn a bundle that fails to open into a blocking `SaveWarning`, with a test.
   - **G1:**
     - Look up the chunk, release the lock, read, then insert. Alternatively, keep one `OnceLock` per chunk, so that two threads don't read the same chunk twice.
     - Recover a poisoned lock with `PoisonError::into_inner`.
     - Bound the cache with a small LRU of parsed chunks, and keep only starts and container summaries, not `components` trees.
   - **G2:** compare the saved block entity's id with the holder's block-entity id, mapping cases such as suspicious sand to `brushable_block`. A mismatch is `Gone`.
   - **G3:** for statuses below `full`, read entities from the chunk's own `entities` list. Confirm with a save that has a proto chunk with a minecart.
4. **Data packs** (P5–P7, G6's fuzz part). Governed by [ADR 0072](../adr/0072-data-packs-as-layers.md) and [data-packs.md](../specs/data-packs.md).
   - **P5:**
     - Read format numbers through `codec::int_of`, which already ports `LazilyParsedNumber.intValue`.
     - Drop the 256-part cut-off, or document it as a limit for hostile input in data-packs.md.
   - **P6:** skip entries with bad paths and give a note naming them. The pack stays readable. Mention `:` in `check_path`'s doc.
   - **P7:** validate each tag file's `values` elements and `replace` in `merged_tag`. Skip a file that fails, with a warning, as the game does.
   - **Confirm against the game:** check P5 to P7 with a test pack each through `cargo xtask oracle test-pack`.
   - **Fuzzing:** extend `fuzz/fuzz_targets/pack.rs` to layer the fuzzed pack over a tiny in-memory bundle, then resolve every file and tag through it.
5. **Tooling and CI** (B7, G7, G8).
   - **B7:** validate the flag combinations before `build()`, and write `packs.json` only after the run succeeds.
   - **G8:**
     - One xtask `base_version` helper, built on `spyglass_oracle::base_version`.
     - One `PACKS_FILE` constant.
     - The no-carvers pack at `target/oracle/<base>/<label>/no-carvers/`.
   - **G7:** add `Cargo.lock` and `rust-toolchain.toml` to `math.yml`'s `paths`, and move `release.yml`'s matrix values into `env:`.
6. **Splitting long code** (S1–S4). Each split is a move with no change in behavior, in its own commit, so that its diff reads as a move. Each is checked with `cargo xtask check`, and the viewer ones by eye with the `run-viewer` skill.
   - **Viewer:**
     - `App::map` becomes tile polling, drawing, input, and overlays (markers, structures, the generated layer).
     - The container window moves to `app/container.rs`.
     - `map_controls`, `describe` and `map_legend` are each split by section.
     - `layers_tab` and `structure_list` are split by group.
   - **`map_features`:** one helper per layer, sharing a helper for the min/max and scale check.
   - **`pack.rs`** becomes a module:
     - `pack/meta.rs`: `parse_meta`, `FormatFields`, `format_value`, `int_range`, `overlays`, `filter`
     - `pack/read.rs`: `PackReader`, `Budget`, `check_path`, `wanted`
     - `pack/mod.rs`: `Pack`, `Layer`

     `FormatFields::validate` is split into its new-fields and old-fields branches.

   - **xtask:**
     - `code_diff.rs`'s method diff and code map (`methods`, `Method`, `Change`, `method_changes`, `arity`, `code_map`) move to `code_map.rs`, beside `citations.rs`.
     - `oracle.rs`'s pack plumbing moves to `oracle/packs.rs`: `use_packs`, `variant_label`, `no_carvers_pack`, `fixture_pack`, `test_pack` and `PACK_MCMETA`.
     - `oracle.rs`'s `run()` and `locate.rs`'s `run()` and `run_map_cases()` are split by step.
     - `math_report::report` is split into reading a set's records and formatting a row.
7. **Close.** Recheck the line counts with a quick count of function and file lengths. Run the full verification below, and record the numbers in the notes.

## Tasks

### 1. Quick fixes and docs

- [x] B1 `nbt::MAX_TEMPLATE` (64 MiB) and `nbt::read_template`; the three readers use it, and report `NbtError::reason`; tests: `templates_are_read_within_their_limit` (a 64 KiB bomb fails with `TooLarge`) and `every_vanilla_template_is_within_the_limit` (ignored, needs a bundle; passes on 26.3); data-packs.md's limits.
- [x] B1 The unlimited `read_compressed` kept, documented as for spyglass's own and the game's files only: its remaining callers are xtask's `fuzz` and `version-diff`, on bundle files.
- [x] B3 `heightmaps` in `has_features`, and the doc names each era's statuses; `handwritten.rs` covers `heightmaps`, `initialize_light`, `spawn` and `liquid_carvers` (fails without the fix).
- [x] B4 `Save::packs` keeps a name's first appearance, as `MinecraftServer.configurePackRepository`'s `LinkedHashSet` does (26.3 bytecode); `current_layout` lists two packs twice (fails without the fix).
- [x] B6 Both tooltips use `\` continuations; a grep for runs of spaces inside literals found no others, so no check was added.
- [x] G5 `bundle::sha1_hex_of(&[u8]) -> String`, used by packs, the extractor's writer and manifest, and the oracle fixtures.
- [x] D1–D10 Doc and comment fixes. D9 names `StaticCache2D.get` (26.3 bytecode).

### 2. `spyglass-math`

- [x] B2 `HUGE_EXPONENT` (2^64): ∞ or 0 before `y` is split; `huge_exponents_overflow_or_underflow`, including bases one ulp from 1; `dd.rs:33` comment.
- [x] P1 A `pow midpoints` set in `JavaMath.java` (40,000 cases; `powExact` made exact for half-integer powers of perfect squares); `y == 2.0` as `x * x`; `exact_power` where the rounding test fails; `squares_are_products`, `midpoints_round_half_to_even`; 40 midpoint cases in `correctly_rounded.rs` (9 of them fail on the old code).
- [x] P2 A `far-edges` set for `sin` and `cos` (the 2,000 multiples of π/2 below 2^20 nearest a double, ±8 ulps; no case wrong before or after); the reduction's error measured for every `k` (2^-90.5 before, 2^-105.1 after); `two_prod` for the third part and a fourth part; the bound in `reduce`'s doc; 80 cases in `correctly_rounded.rs`.
- [x] G6 Special cases for `sin`, `cos`, `log` and `pow` (NaN, ±0, ±∞, subnormals; `exp` is internal and reached through `pow`); `correctly_rounded.rs`'s match names `pow` and its message is escaped.
- [x] S5 `BOUND`, `FACTORIALS`, `inverse_factorials` and `rounded` in `dd.rs`, with compile-time checks that each series fits the table.
- [x] G4 One `Mth.SIN` in `spyglass_noise::mth::sin_table`, built once, for carvers and density functions (noise now depends on math; blocks no longer does); `in_out_sine` 238 ns → 2 ns a call, same results.
- [ ] Oracle math, `math-report` on three platforms; java-math.md counts updated. Done for Windows 11 and Linux x86-64 (Docker); the old sets checked on the CI artifacts of all three. macOS needs the Math workflow's `measure` job.

### 3. Saves

- [x] P3 Confirmed from 26.3's `DataFixers`: `StructuresBecomeConfiguredFix` is schema 2970, and no later fix touches starts. Current-format chunks below 2970, or with no `DataVersion` (read as -1), are `SavedStarts::Legacy`; `chunks_give_their_starts_and_containers` covers 2865, 2969, 2970, 2975 and none; world-saves.md and `your-worlds.md`.
- [x] P4 Confirmed from `unpackStructureStart` and `loadStaticStart`: the key and the `id` must both be registered, a missing `id` reads as `""`, and `ChunkX`, `ChunkZ` and `references` default to 0. `SavedStart::id`; the session drops a start whose key or `id` the world lacks; tests for a start with no position and one with no `id`; world-saves.md.
- [x] B5 The other way round from the plan: a start the save holds is certain, even in a ring alternative (`Starts::stored_decides`, in `possible_starts_in` and both locate paths), so "saved" and the docs were right and locate was wrong. `a_saved_start_is_certain_in_a_ring_alternative` (ignored, needs a bundle; fails without the fix); `StructureHit::certain`, `StructureStart::from_save`, world-saves.md. The JSON is unchanged.
- [x] B8 Closed with no change: see notes.
- [x] G1 `ReadChunks`: a cell per chunk, the map's lock held only to find it, poison ignored, cleared at 16,384 chunks; tests: one read for eight threads on a chunk, eight chunks read side by side, a panicking read retried, the cap. `components` trees are kept: `InSave::Filled` shows them.
- [x] G2 The block report's format 5 gives each block's `block_entity` (`BlockEntity.getType()`'s id); `BlockBehaviour::block_entity`, `Blocks::block_entity`; `SaveChunks::container` takes the holder's type and reads a block entity of another type at the position as `Gone`, or any type with an older report; chunks before data version 3438 read `suspicious_sand` as `brushable_block`, the only block-entity rename since 1.18 (26.3's `DataFixers`). Tests: `a_block_entity_of_another_type_is_gone`, `suspicious_sand_is_renamed_where_the_game_renames_it`, `block_entity_types_are_per_block_from_format_5`; data-sources.md, world-saves.md, `your-worlds.md`. The bundle format stays 1: older bundles load and match by position. Every local bundle re-extracted (26.2, 26.3, 26.4-snapshot-5 to 8, all format 5); in 26.3 copper chests report `minecraft:chest`, every shulker box `minecraft:shulker_box`, suspicious sand and gravel `minecraft:brushable_block` (not loot containers in 26.3, so the rename only keeps ids straight). `saved_containers_have_the_predicted_seeds` on the local saves gives m25's counts exactly: 455 unopened, 3 filled, 13 gone, the same two certain ones gone.
- [x] G3 Confirmed from `SerializableChunkData`: a proto chunk saves `entities` in the chunk. `ChunkContents::entities` and `is_full`; minecarts in a proto chunk come from it; `proto_chunks_hold_their_own_entities`; world-saves.md.

### 4. Data packs

- [x] P5 26.3 loaded a pack declaring `"min_format": 121.0, "max_format": [4294967417, 0.5]`. Format ints through `codec::int_of`; the 256-part limit is the game's (`NON_NEGATIVE_INT.listOf(1, 256)`, bytecode), now said; tests for both forms, a 256-part list, `[]` and a string.
- [x] P6 26.3 logged "Invalid path in pack: minecraft:worldgen/noise/Bad_Name.json, ignoring" and loaded the rest. Such paths are skipped unread, not errors, and so is a namespace or path an `Identifier` can't hold (uppercase among them, which spyglass had loaded); `paths_that_arent_plain_are_skipped_not_errors`, `zips_are_read_within_limits`. No note is kept: the game only logs.
- [x] P7 26.3 logged "Couldn't read tag list minecraft:is_ocean" for `{"values": [3]}` and loaded. `reads_as_tag_file` checks `TagFile.CODEC`'s shape (entries, `replace`, `required`, ids), with numbers as booleans as DFU reads them; `tags_merge_through_packs` covers each. Skipped, as the game skips it; no warning, as above.
- [x] The pack target layers its pack over a small bundle and resolves every entry, tag and template through it (templates through `nbt::read_template`). 300 s, 30,687 runs, no crashes.
- [x] data-packs.md updated for P5–P7.

### 5. Tooling and CI

- [x] B7 `test-pack` refuses `--pack` before `build()`, and `oracle locate` checks `--pack` with `--world` (`locate::check_packs`) before it; `pack_flags_a_command_refuses_are_refused_before_anything_is_written`. `packs.json` stays written at build: a run that fails later still leaves outputs to compare or delete, as without packs.
- [x] G8 `oracle::base_version` and `oracle::PACKS_FILE` used by `locate` and `oracle_test`; the no-carvers pack at `target/oracle/<version>/<label>/no-carvers/` for a variant; oracle.md.
- [x] G7 `math.yml` runs on `Cargo.lock` and `rust-toolchain.toml` too; `release.yml`'s matrix values reach `run:` through `env:` (with `shell: bash` on the steps Windows would run in PowerShell); zizmor passes.

### 6. Splitting long code

- [x] S1 Viewer `map.rs` is now `app/map/` (`mod.rs` 464 lines, `controls.rs` 413, `marker_layer.rs` 557, `jobs.rs` 255, `menu.rs` 202), and the container window is `app/container.rs`. `App::map` is 87 lines over a `Frame` (the frame's keys and tiles), `Canvas`, `Placing` and `Pointed`; `map_controls`, `map_legend` and `describe` are split. Checked by eye with `run-viewer` on 26.3: the map, its controls and legend, structure and feature markers and their tooltips, the container window and the right-click menu.
- [x] S1 `layers_tab` into `biomes_section` and `overlays_section`; `structure_list`'s rows into `structure_family`, and its row and variants into `family_row` and `variant_checkbox` (checked by eye: the mixed checkbox, a family on, its variants opened, a variant on).
- [x] S2 `map_features` calls `map_structures`, `map_feature_layer` and `map_containers`, sharing `check_layer_scale`, `area_chunks` and `reach_chunks`; every error message is unchanged.
- [x] S3 `pack/mod.rs` (types, `Pack`, `Layer`), `pack/read.rs` (the reader, its budget and path rules), `pack/meta.rs` (`pack.mcmeta`), each with its tests; `validate` into `validate_new`, `validate_old` and `check_pack_format`, over a `Holder`.
- [x] S4 `code_diff/methods.rs` (method diffing; `code_map` stayed, split into `cited_entry` and `ranked_report`, since its helpers are shared with `unused_report`); `oracle/packs.rs` and `oracle/build.rs` (oracle.rs 1,156 → 732 lines); `locate::run` into `save_job` and `write_answers`, `run_map_cases` into `place_map_chest` and `map_answer`; `math_report::report` into `read_set`, `tally_set` and `cells` (the report's output unchanged). `oracle::run` stays one `match` over 20 subcommands, with a `seeds_or` helper. `locate.rs`'s explorer-map cases moved to `locate/maps.rs` (`locate.rs` 946 → 645 lines).

### 7. Close

- [x] Length counts rechecked; any function left over 80 lines named in the notes with its reason.
- [ ] Verification below run and recorded.

## Verification

- Every parity and bug fix has a test that failed before it. The notes name the test for each.
- `cargo xtask check` passes after every phase.
- `cargo xtask oracle-test` passes for every supported version after phases 2, 3 and 4.
- `cargo xtask oracle math` and `cargo xtask math-report` show no case wrong and no more open cases than java-math.md records, on Windows, Linux x86-64 and Apple silicon.
- `cargo xtask bench`: no regression beyond noise on the main operations, compared back to back with the review's commit.
- Viewer: the map, the generated layer, the container window and the tooltips are checked by eye after phase 6.

## Durable outputs

Check these before marking the plan complete, since the plan will be archived and code can't reference it.

- [ ] [java-math.md](../specs/java-math.md): midpoints and integer powers, huge exponents, the trig reduction and its bound, and the new counts.
- [x] [world-saves.md](../specs/world-saves.md): starts in 1.18.0 and 1.18.1 chunks, the presence rule, when a saved start is certain, entities in proto chunks, and containers matched by block-entity type.
- [x] [data-packs.md](../specs/data-packs.md): format numbers, bad paths and malformed tags.
- [x] [cli-json.md](../specs/cli-json.md) and the user guide, if B5 changes what is printed. B5 changed neither the text nor the JSON; `your-worlds.md` covers P3 and G2.
- [x] An ADR only if a fix changes a decision. None did: G2's report field follows ADR 0054's pattern, a report format with older bundles still read.

## Notes

Decisions, surprises and dead ends, newest first.

- 2026-11-14, `oracle-test` after phases 2 to 4: every version and variant passes (26.2, 26.3 and the four snapshots, each with `+overrides`, and `26.4-snapshot-8+testpack`) but `26.3+terralith`. There, 32 of 57 tests fail, every one on the three unsupported files data-packs.md names, one more than m26 counted; the count there is updated.
- 2026-11-13, phase 7:
  - **Lengths, by a brace-matching count of non-test code.** Of the functions the review named, these are still over 80 lines:
    - `App::map` (87), `draw_marker_layer` (88, split out of it) and `container_window` (86): each is one pass over one frame's drawing, and splitting them further would pass most of their state through.
    - xtask `oracle::run` (126): one `match` arm per subcommand, each a call or two.

    Every other function the review named is now under 80 lines. No file the review named is over 750 lines. The count finds about 70 other functions over 80 lines, all older than the review and outside it, most of them close ports of one game method (`sampler.rs`'s `compute`, `mansion.rs`'s `floor`), which follow the game's structure on purpose.

  - The B6 tooltips sit on the generated-chunks checkboxes, which are enabled only with a world open. The viewer opens one through the system's file dialog, which `run-viewer` can't drive, so they weren't seen on screen; their literals were checked in the source.

- 2026-11-13, phase 4:
  - P5 to P7 were checked in one run of the 26.3 harness on a hand-made pack (`--pack <dir> nothing nowhere`, which loads the game and stops): `"min_format": 121.0` with `"max_format": [4294967417, 0.5]`, `is_ocean` as `{"values": [3]}`, and `worldgen/noise/Bad_Name.json`.
  - **A new fault from P6's check:** the game skips a file whose path has uppercase letters, and spyglass loaded it. A pack's `Structure.json` would have been placed by spyglass and not by the game. Fixed with P6.
- 2026-11-13, phase 3:
  - **B5 went the other way.** The plan was to show "saved" only where locate called a start certain. But a ring alternative is uncertain only because spyglass can't tell which chunk the game's platform computed, and a saved start answers exactly that. So locate now trusts a chunk the save decides, and the docs' "a saved start is certain" holds.
  - **B8 needed no change.** `inspect` already failed on a bundle that won't open before this branch: `inspector.presets(bundle)?` opens it first. The new `Bundle::open` after it opens the same folder again, so it can only fail if the bundle changes in between.
  - Phase 2's `oracle-test` was still running while phase 3 was edited, so its later versions may have built phase 3's code. Phase 4's run covers both.

- 2026-11-13, phase 2:
  - **Java's `pow` isn't correctly rounded at midpoints**, so P1's target is Java's answer, not half to even. Measured with JDK 25 on 400,000 cases (JIT and `-Xint` alike): squares always match `x * x`, which is HotSpot's special case; cubes, fourth powers and 1.5th powers differ from half to even in about 10%. spyglass now matches `x * x` for squares, and rounds the other midpoints half to even, which is as close as it can get without porting Intel's `pow`: on the midpoint set it differs from Java in 6.58% (Java's own misses), down from 20.38%.
  - **P2 was real but unobserved.** The review's estimate held: the reduction's worst relative error, 2^-90.5, was past the 2^-95 the rounding test assumes. It was measured by porting `reduce` op for op to Python (whose floats are IEEE doubles) and comparing with an exact remainder for every `k` below 2^20/(π/2), at the doubles nearest each `k·π/2`. No result in the far-edges set was wrong even so.
  - The new sets add under a second to `oracle math`, 33 s in all on this machine.

- 2026-11-13, phase 1: the template cap is 64 MiB, like a save's NBT files (`MAX_NBT_FILE`), against about 1 MiB for the largest vanilla template (`bastion/hoglin_stable/air_base`, measured on every local bundle). The same figure bounds the tree's memory, and every vanilla template reads within it. The game reads templates with `NbtAccounter.unlimitedHeap`, so a pack's template over 64 MiB is a deviation, recorded in data-packs.md.
