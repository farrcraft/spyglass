# m10. Hardening after the 2026-10-28 audit

- Status: In progress
- Milestone: [10. Hardening](../roadmap/10-hardening.md)
- Started: 2026-10-28
- Completed: —

## Goal

Fix or close every finding of the repo audit of 2026-10-28. This covers all of milestone 10's exit criteria.

The audit found no bit-exactness bugs and no violations of the project rules, and `cargo xtask check` passed. Its findings are:

- robustness against hostile saves and data packs
- search, locate and the viewer not stopping promptly
- tooling and supply-chain gaps
- documentation drift
- maintainability debt
- the tests that would have caught each of these

## Findings

From the audit. Findings marked _(confirm)_ were reported as plausible: the first task for each is a test that shows the failure, and if it doesn't fail, the finding is closed in the notes with that test kept.

- **Hostile input**
  - H1. `nbt.rs:354` reserves `len.min(self.bytes.len())` list elements, a cap set by the whole buffer, not by the bytes left. One list header in a ~64 KiB gzip `level.dat` reserves ~2 GiB (64 Mi × `size_of::<Tag>()`), and nested lists multiply it. Allocation failure aborts.
  - H2. _(confirm)_ Density-function references chain across files with no depth limit. `load.rs:80-140`, the binders, `Sampler::eval` and dropping the `Arc` tree all recurse, so a pack with thousands of chained references overflows the stack. Cycle detection scans `loading` linearly (`load.rs:85`), O(n²) on long chains.
  - H3. _(confirm)_ `sampler.rs:1801` finds a spline interval with `partition_point`, whose result is unspecified on an unsorted slice. The game uses `Mth.binarySearch`, and the codec doesn't require ascending locations.
  - H4. `region.rs:149` accepts `length > sectors * 4096 - 4` up to `sectors * 4096`. The game's `RegionFile` treats those chunks as truncated, so the unreadable counts can differ from the game's.
  - H5. Region scanning reads each file whole on every core. Peak memory is cores × file size, with the cap near 1 GiB per file. One file that can't be opened fails the whole scan instead of counting as unreadable.
  - H6. _(confirm)_ `decode_modified_utf8` (`nbt.rs:458-479`) rejects a raw `0x00` and lone surrogates, which Java's `readUTF` accepts. A `LevelName` with a lone surrogate fails all of `Save::read`.
  - H7. _(confirm)_ Empty jigsaw pool-alias `targets` and `groups` load (`jigsaw.rs:1225-1242`), then `pick` panics in `next_int_bounded(0)` (`assembly.rs:177`).
  - H8. `codec.rs:242` converts `BigDecimal.intValue` as `(d.trunc() as i64) as i32`. That saturates for |x| ≥ 2⁶³, where Java keeps the low 32 bits of the exact integer. The comment claiming this covers every accepted range is wrong.
  - H9. `Save::region_dir` (`spyglass-world/src/lib.rs:409-427`) joins a dimension id's namespace and path into a filesystem path unchecked. Today's callers pass only built-in dimensions.
  - H10. Public library functions assert on caller input instead of returning errors:
    - `spyglass-biome/src/locate.rs:146`: steps must be positive
    - `source.rs:364`: `increment > 0`
    - `source.rs:449-453`: `.expect` on size overflow, then an unbounded `vec!`
  - H11. Some data hangs the evaluator exactly as it hangs the game: 26.2's `new_legacy` with `firstOctave` near −2³¹, and `find_surface` with an upper bound of +∞. This is parity, but it isn't documented.
- **Responsiveness and CLI**
  - R1. `crates/spyglass/src/search.rs:548-566` re-evaluates every top hit after the search, one at a time, with no way to stop. `--top 10000` on a stronghold query spends hours after Ctrl-C, and a second Ctrl-C does nothing. The viewer's Stop is the same.
  - R2. _(confirm)_ `spyglass-search/src/run.rs:281-283` sizes the next round from the last one's speed, up to 2³⁰ indices.
    - A round whose groups were all rejected is fast, so the next round is huge. `--time` and Stop then wait for it.
    - Its `batches` Vec can reach ~2 GB.
    - ADR 0021 says progress runs "between batches"; it runs between rounds.
  - R3. The viewer resolves the query in `ui()` on every edit (`app.rs:786-793`). `Scope::infer` can call `Game::world`, which loads the Nether or End bundle on the UI thread.
  - R4. Locate radii have no upper bound in the library or the CLI (`lib.rs:1004-1017`, `args.rs:719`). `locate slime --radius 100000` walks 4×10¹⁰ chunks and collects billions into a Vec.
  - R5. `Session::heights_in` (`lib.rs:777`) computes the width in `i32`, which overflows. It has no size cap outside the CLI.
  - R6. `run.rs:185-188` panics with `.expect("a thread pool")` if the pool can't be built.
  - R7. `args.rs:539-540` computes `n * 60` and `n * 3600` unchecked.
  - R8. All CLI output goes through `println!`, so `spyglass … | head` panics on the broken pipe (exit 101).
  - R9. Viewer workers:
    - A panic in a tile worker (`tiles.rs:161-182`) leaves its job in `running` for good, so the tile never draws and the pool loses a worker.
    - A new locate or search orphans the previous worker thread.
    - Two threads can load the same world (`app.rs:150-163`, `game.rs:63-67`).
  - R10. String literals hold raw newlines where `\n` was meant, and in places the next line's indentation became part of the string:
    - `spyglass-cli/src/locate.rs:106,111` print 14-space gaps.
    - `xtask/src/locate.rs:338-362` includes an indented `server.properties` line.
    - Same style: `spyglass-search/src/parse.rs:683-717`, `xtask/src/version_diff.rs:90,368`, `xtask/src/code_diff.rs:491`.
- **Tooling and supply chain**
  - T1. `xtask/src/locate.rs:336-339` writes `eula=true` for every throwaway server, accepting Mojang's EULA for whoever runs the oracle. No doc says so, and ADR 0004 and `research/oracle-approaches.md` imply otherwise.
  - T2. `rust-toolchain.toml` floats on `stable`. With `clippy -D warnings`, a new Rust release can fail CI with no code change, and local and CI toolchains drift.
  - T3. `manifest.rs:125` never checks the version JSON against `VersionEntry.sha1`, and that JSON supplies the jar's hash and URL. ureq follows redirects without `https_only`, and the jar URL's scheme isn't checked.
  - T4. `linux_check.rs:227,252,255-257` puts the game version unquoted into `sh -c`.
  - T5. `setup.sh:29,37` passes `$(cargo xtask msrv --print)` straight into a command, so `set -e` doesn't see it fail. `setup.ps1` checks every exit code.
  - T6. Pre-commit hook repos are pinned by tag, not commit. `uv tool install pre-commit` is unpinned.
  - T7. CI:
    - It runs on Linux only, though Windows is the main development OS and `setup.ps1` and the Windows Java discovery are untested.
    - It has no `concurrency:` group.
    - `setup.sh` recompiles nextest and cargo-shear on every run.
    - Dependabot covers only github-actions.
  - T8. `rust-version` and `channel` are read by two separate line-based TOML parsers (`xtask/src/main.rs:193-211`, `linux_check.rs:120-129`).
  - T9. `deny.toml` bans nothing (for example `openssl`, which rustls makes unnecessary).
  - T10. The `no-process-doc-refs` hook covers only `crates/` and `xtask/`, but the rule also covers `scripts/`, and `oracle/` is code too.
  - T11. `.devcontainer/Dockerfile` pins `ubuntu:24.04` by tag, not digest.
- **Docs**
  - D1. `specs/density-functions.md:28,257,393` link `noise.md#normal-noise`; the anchor is `#normal-noise-normalnoise`.
  - D2. Completed plans:
    - Broken links: `m06-seed-search.md:4` (one `../` too many), `m08-dimensions.md:273` (`../m08-versions.md`), `m02-biomes.md:71` (`#column-evaluation` is gone).
    - `m03-spawn.md` is completed but says `Status: Draft`, with no dates.
  - D3. `architecture.md`:
    - The code-vs-data table has a "Features" row for a layer that doesn't exist.
    - The `spyglass-structure` row omits its `density` and `noise` dependencies.
  - D4. The roadmap row for 09 says "flattened, SIMD-friendly", which ADR 0032 superseded.
  - D5. Surface rules, features and the final spawn block are "deferred to a later milestone" (`specs/spawn.md`, milestone 07), but no roadmap entry tracks them.
  - D6. `spyglass-terrain`'s crate doc says "the Overworld's generator"; the crate covers all three dimensions.
  - D7. Command lists:
    - `cargo xtask version-diff` is missing from CLAUDE.md and getting-started §5.
    - `cargo xtask oracle` is missing from CLAUDE.md.
    - Getting-started §6 doesn't list the `oracle` subcommands.
    - `xtask/Cargo.toml`'s description lists six commands of ten.
  - D8. ADRs 0031 and 0032 name the m09 plan in prose.
  - D9. README:
    - Its quick start doesn't say the extractor needs a matching Java.
    - Its documentation list omits `adding-a-version.md` and `plans/`.
  - D10. The repo has no CONTRIBUTING.md or SECURITY.md.
  - D11. `cli-json.md` doesn't list the `{"kind":"other"}` feature kind that `json.rs:377` emits.
  - D12. Nothing checks Markdown links, so D1 and D2 went unnoticed. `versions.rs`'s anchor test shows the approach works.
- **Maintainability**
  - M1. `spyglass-viewer/src/app.rs`:
    - It is ~2,000 lines, and `App` has ~40 fields.
    - `map()` and `search_section()` are each 200–330 lines.
    - It has one test.
  - M2. Code is repeated between evaluator families:
    - the permutation shuffle ×4 (`perlin.rs:60`, `simplex.rs:20`, `v1_21/improved.rs:49`, `v1_21/simplex.rs:518`)
    - 2D simplex with `corner`/`dot`
    - legacy octave construction (`normal.rs:286`, `v1_21/perlin.rs:295`)
    - `NETHER_BIOME_NOISES`, `WorldRandom` and `BINDERS` (`sampler.rs`, `v1_21/bind.rs`)
    - `Node::kind` and `point_kind`
  - M3. Public types can't grow without breaking callers:
    - These have all-public fields and no `#[non_exhaustive]`: `SearchOptions`, `LocateOptions`, `Layers`, `MapArea`, `StructureStart`, `SearchOutcome`, the `Outcome` enum and the query AST (`Kind`, `Region`, `Place`).
    - `spyglass::Kind` and `spyglass::query::Kind` share a name.
    - `Query::forget_dimension` and `SearchQuery::dimension` panic on a bad index.
  - M4. `sampler.rs:1502-1512`: `depends_on_y`'s doc is merged into `position_only`'s, and `depends_on_y` (`:1575`) has none.
  - M5. `sampler.rs:887-888` uses Rust's `f32::min`/`max` in 26.3's `end_island_height`, where the 26.2 twin uses `java::max` and `mth_clamp_f32`. It's harmless for finite values, but uncommented.
  - M6. `spyglass-noise`'s `Volume` has public fields that skip `new`'s checks (the density crate builds it as a literal), `is_empty()` is hard-coded `false`, and `index()` can overflow `i32`.
  - M7. `LoadError::new("", "", …)` for `IntervalError` and anchor resolution (`sampler.rs:337-343`, `:932-936`) names no function or JSON path.
  - M8. `IntervalError::reason` duplicates its `#[error]` strings (`interval.rs:17-36`); `NoiseError` shows the single-source pattern.
  - M9. `domain_axes` runs twice per `domain_warp` with a fresh `HashMap`, which is quadratic for nested warps (`sampler.rs:645-646`). `range.rs:19-27` rebuilds a `NormalNoise` per noise range.
  - M10. The CLI and viewer each compute a feature's map position (`cli/src/search.rs:408-416`, `viewer/src/search.rs:186-218`) and each hold the "Candidates are …" explanations. The library should own both.
- **Tests**
  - X1. Missing NBT cases:
    - a huge list length
    - nesting at depth 512 and 513
    - raw `0x00` and lone surrogates in strings
  - X2. There are no fuzz targets for `nbt::read` or `region::file_versions`.
  - X3. Values pinned from non-Mojang sources are missing for:
    - Xoroshiro's `next_int_bounded`, `next_float` and `next_double`, which have only range tests
    - 26.3 `PerlinNoise::get`, which has no hand-worked value
    - 26.3 `NormalNoise::create`'s layer interleaving and `f32` amplitude narrowing
  - X4. The CLI's `duration`, `range`, `size`, `origin` and `xz` parsers have no unit tests.
  - X5. Nothing tests stopping a search during re-evaluation, or round sizing after a fully rejected round.
  - X6. Nothing tests the CLI's human-readable `locate` output.

## Approach

Six phases, each one or more commits that pass `cargo xtask check` on their own, and `cargo xtask oracle-test` where they touch worldgen. Text and one-line fixes come first, the input hardening that has the most risk comes next, and refactors come last so that they move code that is already fixed.

1. **Docs, links and literals** (D1–D12, R10). Mostly text.
   - **Link check:** a new `cargo xtask links`, run as a commit hook. It checks every relative link and anchor in tracked Markdown using GitHub's heading-anchor rules.
     - It is written in Rust in xtask (CLAUDE.md), with the placeholder links in templates and `.claude/skills/` allowed by an explicit exclude list.
     - It lands with the fixes so that the hook passes from its first commit.
   - **Deferred work:** surface rules, features and the final spawn block go into the roadmap README under a new "Not yet planned" list.
   - **ADRs 0031 and 0032 stay as written:** ADRs aren't rewritten (ADR 0001), and they name the plan in prose rather than linking it. This is recorded in the notes and closes D8.
2. **Hostile input** (H1–H11, X1, X2). Each fix lands with a test that fails before it. This follows the m05 pattern.
   - **NBT:**
     - List reservation becomes `len.min(remaining / min_payload_size(element))`, where a TAG_End or compound element counts as 1 byte.
     - Strings decode as Java does. Raw `0x00` and lone surrogates are accepted, and stored lossily (U+FFFD) for values. Names keep strict decoding, because a lossy name could collide with another key.
     - `world-saves.md` says so.
   - **Density depth:**
     - The loader carries a reference depth and fails with a `LoadError` past a fixed limit. The limit is the game's if it has one, otherwise one high enough for every vanilla graph. The vanilla maximum is measured on every supported bundle and recorded in `density-functions.md`.
     - Binding, evaluation and drop then see only bounded trees.
     - `loading` becomes a `HashSet` alongside the ordered stack used for error messages.
   - **Splines:** port `Mth.binarySearch` as `mth::binary_search` and use it in both families. A test with unsorted locations runs through both paths.
   - **Region files:**
     - Match `RegionFile`'s length check.
     - Read the 4 KiB header, then seek to each chunk instead of reading the file whole.
     - An unopenable file counts its chunks as unreadable and adds a warning.
     - `world-saves.md` records the rule.
   - **Jigsaw aliases:**
     - Check the game's codec (bytecode via `cargo xtask version-diff --code`, or a test pack through the oracle) for whether it rejects empty `targets`/`groups`. Reject them at load if it does.
     - If the game loads them and crashes at generation, refuse them at load anyway and record the deviation in `structure-placement.md`.
   - **`int_of`:** exact low-32-bit conversion from the number's text, as Java's `BigDecimal.intValue` does: parse the integer part as `i128`, or wrap through the digits when it doesn't fit.
   - **`region_dir`:** validate the id with `Bundle::resource_path`'s rules, and return `WorldError` on failure.
   - **H10:** `find_closest_biome_3d`, `find_biome_horizontal` and the region fill return a `Result`, or take a type that can't hold a bad value (`NonZeroU32` steps). The front ends keep their own messages.
   - **H11:** no code change. `density-functions.md` gets a "Data the game also can't finish" note listing both cases. Each front end already runs locates off the UI thread and can be stopped (phase 3).
   - **Fuzzing (X2):**
     - `cargo-fuzz` targets for `nbt::read_compressed_limited` and `region::file_versions`, plus a `cargo xtask fuzz <target> [--time]` command.
     - `cargo-fuzz` needs a nightly toolchain, which the project doesn't otherwise use, so an ADR records the choice. Fuzzing runs on demand and in a weekly CI job, not in the hooks.
     - If the ADR is rejected, the fallback is a seeded property test that mutates the committed synthetic files, run in nextest.
3. **Responsiveness** (R1–R9, X4–X6).
   - **Re-evaluation:**
     - `search` re-evaluates hits in parallel on the search's own pool.
     - It checks the stop flag between hits, and `progress` gets a phase field (`Searching`/`Confirming { done, of }`).
     - A stopped re-evaluation returns the hits evaluated so far, marked as partial in `SearchOutcome`.
     - The CLI's second Ctrl-C exits at once with code 130.
     - `search.md` and `cli-json.md` describe the partial result.
   - **Rounds:**
     - Cap a round's duration, not just its size: the next round is sized from the slower of the last round and the running average.
     - Groups are checked in chunks, with the stop flag polled between chunks, so a single huge round can still stop.
     - `batches` is bounded by chunking.
     - ADR 0021's "between batches" is corrected by a new ADR only if the design changes. Otherwise `search.md` states the actual stop granularity.
   - **Viewer query:**
     - Parsing stays on the UI thread.
     - Resolving the query against worlds runs through `Pending` like every other load. Until it lands, the editor shows "loading the Nether…".
     - `Game::world` loads each dimension once (`OnceLock` per dimension), which removes the duplicate loads.
   - **Bounds:**
     - The library caps locate radii at the game's own limits where it has them (`/locate`'s search radius, `/locatebiome`'s 6400). Elsewhere it uses a documented maximum, with `Error::TooLarge { what, max }`.
     - `heights_in` uses `i64` for the region size, with the same cap as the CLI moved into the library.
     - `locate.md` lists the caps.
   - **CLI:**
     - Output goes through a locked `stdout` writer, and `BrokenPipe` ends the process quietly with exit 0.
     - `--time` uses `checked_mul`.
     - A failed thread pool becomes `Error::Threads`.
   - **Viewer workers:**
     - Each job runs under `catch_unwind` and reports a panic as an error tile.
     - Locate and search hold a cancel flag that a new request sets before starting the next worker.
4. **Tooling and supply chain** (T1–T11).
   - **EULA:**
     - Commands that start a server require `--accept-eula` or `SPYGLASS_ACCEPT_EULA=1`. Without either they stop with a message that links the EULA. CI sets the variable, with a comment.
     - A new ADR records that the oracle runs the server under the user's acceptance, and refines ADR 0015.
     - `oracle.md` and getting-started describe it.
   - **Toolchain:**
     - `rust-toolchain.toml` pins an exact release, 1.99.1 today. The MSRV stays separate.
     - Bumps are deliberate: Dependabot doesn't update this file, so `cargo xtask doctor` reports when a newer stable exists and the bump is a normal commit.
     - An ADR records the policy.
   - **Downloads:**
     - The version JSON is checked against `VersionEntry.sha1` before use.
     - The ureq agent is built with `https_only(true)`.
     - The jar URL must be `https`.
     - `data-sources.md` states the chain of trust.
   - **Scripts and linux-check:**
     - Pass the version as a positional argument (`sh -c '… "$1"' sh <version>`).
     - `setup.sh` assigns `msrv=$(…) || exit` before using it.
     - `uv tool install pre-commit==<version>` in both scripts.
   - **Hooks:**
     - Pin each repo by commit SHA with the tag in a comment, as the workflow's actions are.
     - Widen `no-process-doc-refs` to `^(crates|xtask|scripts|oracle)/`.
     - `development-process.md` matches.
   - **CI:**
     - Add a `windows-2025` job running `scripts/setup.ps1` and `cargo xtask check`. Oracle jobs stay Linux-only.
     - Add a `concurrency` group that cancels superseded runs on the same ref.
     - Install nextest and cargo-shear from prebuilt binaries (`cargo binstall` with pinned versions or the vendors' release archives, checked by hash), or cache `~/.cargo/bin` keyed on their versions. Nothing game-derived is cached (ADR 0008).
     - Dependabot gets `cargo` (weekly, 7-day cooldown, grouped) and `docker` for the dev container.
     - ADR 0028 is refined by a new ADR if the Windows job changes its decision.
   - **Smaller items:**
     - One `toml_value(path, table, key)` helper in xtask, using the `toml` crate as an xtask-only dependency. Its license must be allowed in `deny.toml`.
     - `deny.toml` `[bans] deny = [openssl, openssl-sys, native-tls]`.
     - The Dockerfile pins its base image by digest, kept current by Dependabot's docker ecosystem.
5. **API and maintainability** (M1–M10).
   - **M3 types:**
     - Result types the library builds (`Outcome`, `SearchOutcome`, `StructureStart`) become `#[non_exhaustive]`.
     - Option types (`SearchOptions`, `LocateOptions`, `Layers`, `MapArea`) also become `#[non_exhaustive]`, with `Default` and `with_*` setters so front ends don't build them as literals.
     - The query AST enums become `#[non_exhaustive]`.
     - `query::Kind` is renamed `query::Constraint`.
     - `forget_dimension` and `dimension` take a typed index or return `Option`.
     - Each breaking change is workspace-internal (nothing is published).
   - **M10:**
     - `Feature::map_block()` gives a feature's map position.
     - `Locate::why_uncertain()` returns the explanation as data (an enum the front ends word). The CLI and viewer both use them.
   - **M1:** split `app.rs` into `search.rs`, `locate.rs` and `saves.rs`, each holding its own state and UI panel, and `map.rs` for the map view. Pure helpers move under unit tests. No behavior change.
   - **M2:**
     - Share only the code that is identical to the bit between families: the permutation shuffle, 2D simplex with `corner`/`dot`, and the binder tables, in a `common` module under each crate.
     - Code that differs by width or narrowing stays duplicated, as ADR 0026 intends.
     - `Node::kind` and `point_kind` merge behind one table with a prefix.
     - Every move is checked by `oracle-test` and the bench, which must not regress more than noise.
   - **M4–M9:** local fixes.
     - `Volume`'s fields become private, with accessors. Its `len` is `usize`, computed by checked multiplication, and `is_empty` is real.
     - `LoadError`s carry the function id and path.
     - `IntervalError` gets the single message source.
     - `domain_axes` is computed once per node.
     - The noise for a range is built once per id.
     - `end_island_height` uses the `java::` helpers, with a comment.
6. **Test gaps** (X3, and anything earlier phases didn't cover).
   - Xoroshiro values pinned from the Xoroshiro128++ reference implementation, as ADR 0008 allows.
   - A hand-worked 26.3 `PerlinNoise::get` value.
   - A `NormalNoise::create` test that pins the interleaving and narrowing from first principles.

## Tasks

### 1. Docs, links and literals

- [x] `cargo xtask links`: every relative link and anchor in tracked Markdown, GitHub anchor rules, explicit excludes; a commit hook in `.pre-commit-config.yaml` and so in `cargo xtask check`.
- [x] D1 `density-functions.md` anchors.
- [x] D2 Links in `m06-seed-search.md`, `m08-dimensions.md` and `m02-biomes.md`; `m03-spawn.md` status `Completed` with its dates from git (d6c0c58).
- [x] D3 `architecture.md`: Features row marked planned; `spyglass-structure`'s dependencies.
- [x] D4 Roadmap row 09: "flattened density programs where they pay".
- [x] D5 Roadmap README: "Not yet planned": surface rules, features, the final spawn block; `spawn.md` and milestone 07 link the index.
- [x] D6 `spyglass-terrain` crate doc covers all three dimensions.
- [x] D7 CLAUDE.md and getting-started §5: `version-diff`, `oracle`; getting-started §6: the `oracle` subcommands; `xtask/Cargo.toml` description.
- [x] D8 Closed with no change: see notes.
- [x] D9 README: Java note in the quick start; `adding-a-version.md` and `plans/` in the documentation list.
- [x] D10 `CONTRIBUTING.md` (setup, hooks, the process, the Mojang-data rule, pointing at existing docs rather than repeating them) and `SECURITY.md` (what counts: hostile saves and packs, downloads; how to report). Before the repo is published.
- [x] D11 `cli-json.md`: the `other` feature kind.
- [x] R10 Every multi-line non-raw literal rewritten with `\n` escapes or `\` continuations (the deliberate text block in `code_diff.rs` stays); the CLI's two messages and `server.properties` fixed.
- [ ] X6 A test for the `locate structure` text: moved to M10, which gives the explanation a home in the library where a test can reach it.

### 2. Hostile input

- [x] H1 + X1 NBT list reservation (at most 1,024 up front) and a memory limit on limited reads; tests: a huge list length, a list of a million bytes, nesting at 512 (reads) and 513 (errors), the deepest on a 1 MiB stack.
- [x] H6 + X1 Modified UTF-8 as Java; tests for `0x00` and a lone surrogate in a value and in a name; `bundle-format.md`.
- [x] H2 Test: a chain of 10,000 references (confirmed: it overflowed the stack). Nesting limit of 128 through references; vanilla maximum measured and recorded in `density-functions.md`. `loading` stays a `Vec`: with chains capped at 128, its scan is cheap.
- [x] H3 Test: unsorted locations (confirmed: `[0, 1, 2, 4, 3]` at 3.5 interpolated where the game extends). `java::mth_binary_search`, shared by both families; `density-functions.md` updated.
- [x] H4 Region chunks read as `RegionFile.getChunkDataInputStream` (26.3 bytecode); test at the boundary (fails on the old code).
- [x] H5 Header-then-seek region reads, no whole-file cap; unreadable files counted, with an `unreadable_regions` warning; tests.
- [x] H7 Both versions' codecs use `WeightedList.nonEmptyCodec`: empty lists and sums past `i32::MAX` rejected at load, zero weights accepted, all-zero lists refused (a deviation, in `structure-placement.md`); test.
- [x] H8 `int_of` exact for any number text; tests at ±2⁶³ and beyond; comment fixed.
- [x] H9 `region_dir` validates the id and returns a `Result`; test with `x:../../..`.
- [x] H10 Closed with no change: see notes.
- [x] H11 `density-functions.md`: data the game can't finish either.
- [ ] X2 ADR for fuzzing on nightly; `fuzz/` targets for NBT and regions; `cargo xtask fuzz`; weekly CI job. Or the property-test fallback.

### 3. Responsiveness

- [x] R1 Parallel re-evaluation on the search's pool (`spyglass_search::map_prefix`), stopped by `SearchStop::stop_now`; partial outcomes; a second Ctrl-C or Skip evaluating in the viewer; `search.md`, `cli-json.md`. Test (X5): a second stop gives a partial outcome, a first one doesn't.
- [x] R2 Rounds cut before any seed on a stop, the time limit or a second of running, keeping the index-ordered prefix; at most 2²² checked indices per round; `search.md` on stop granularity. Tests: a stop mid-round, and a time limit after a run of rejected groups.
- [x] R3 Query resolution through `Pending`; `Game::world` loads each dimension once.
- [x] R4 Radius caps in the library, `Error::TooLarge`; `locate.md#limits`.
- [x] R5 `heights_in` in `i64`, saturating, capped in the library and returning a `Result`; test at `i32::MIN..=i32::MAX`.
- [x] R6 `Error::Threads` instead of the `expect`.
- [x] R7 `--time` with `checked_mul`; X4 unit tests for `duration`, `range`, `size`, `origin`, `xz`.
- [x] R8 `out!` writes through locked stdout; a broken pipe exits 0; test reading one line and closing the pipe.
- [x] R9 `catch_unwind` per tile job; a dropped search stops; a replaced locate runs out, bounded by R4 (see notes).
- [x] Exit criterion: `--time` ends a `--top 1000` stronghold search within 20 ms; numbers in the notes.

### 4. Tooling and supply chain

- [ ] T1 `--accept-eula` / `SPYGLASS_ACCEPT_EULA`; CI sets it; ADR; `oracle.md`, getting-started.
- [ ] T2 Exact toolchain pin; `doctor` reports newer stable; ADR.
- [x] T3 Version JSON hash check; one HTTPS-only agent for every download, redirects included; `data-sources.md#chain-of-trust`; tests.
- [x] T4 linux-check quotes the version for the shell (`sh_quote`); test.
- [x] T5 `setup.sh` assigns each command substitution before use, so `set -e` sees failures; pre-commit 4.7.0 pinned in both scripts.
- [x] T6 Hook repos pinned by commit, tag in a `frozen:` comment; bumped with `pre-commit autoupdate --freeze`.
- [ ] T7 Windows CI job (waiting on a decision); done: `concurrency`, `push` on `main` only, cached cargo tools, Dependabot `cargo` and `docker`.
- [x] T8 One `toml_string` helper in xtask, on `toml_edit` (already in the tree).
- [x] T9 `deny.toml` bans `openssl`, `openssl-sys` and `native-tls`.
- [x] T10 `no-process-doc-refs` over `scripts/` and `oracle/`; `development-process.md`.
- [x] T11 Dockerfile base image by digest, kept current by Dependabot.

### 5. API and maintainability

- [ ] M3 `#[non_exhaustive]` on results, options (with `Default` and setters) and the query AST; `query::Constraint`; no panicking index methods.
- [ ] M10 `Feature::map_block`, `Locate::why_uncertain`; CLI and viewer use them.
- [ ] M1 `app.rs` split into `search`, `locate`, `saves` and `map` modules; tests for the pure helpers.
- [ ] M2 Shared identical noise and binder code; merged kind tables; oracle-test and bench before and after, in the notes.
- [ ] M4 `position_only` and `depends_on_y` docs.
- [ ] M5 `end_island_height` with `java::` helpers.
- [ ] M6 `Volume` private fields, `usize` length, real `is_empty`.
- [ ] M7 `LoadError` context for intervals and anchors.
- [ ] M8 `IntervalError` single message source.
- [ ] M9 `domain_axes` once per node; range noises built once.

### 6. Test gaps

- [ ] X3 Xoroshiro reference values for bounded ints, floats and doubles.
- [ ] X3 26.3 `PerlinNoise::get` hand-worked value.
- [ ] X3 26.3 `NormalNoise::create` interleaving and narrowing.

## Verification

- Each fix in phases 2 and 3 has a test that fails before it. A _(confirm)_ finding whose test passes before any change is closed in the notes, and the test is kept.
- `cargo xtask check` and `cargo xtask oracle-test` for every supported version after each phase. Phases 2 and 5 touch worldgen.
- `cargo xtask bench` before phase 2 and after phase 5. No line may regress beyond run-to-run noise, and the numbers go in the notes.
- The fuzz targets run for an hour each with no crash before the plan completes.
- The stop exit criterion is timed by hand on both the CLI and the viewer.
- `cargo xtask links` passes, and fails on a deliberately broken link.

## Durable outputs

- [ ] ADR: fuzzing with `cargo-fuzz` on nightly (or the rejected alternative).
- [ ] ADR: the oracle accepts the EULA only on explicit request (refines 0015).
- [ ] ADR: exact toolchain pin and how it's bumped.
- [ ] ADR: Windows in CI, if it changes ADR 0028.
- [ ] ADR: search stop granularity, only if the round design changes ADR 0021.
- [ ] `density-functions.md`: reference depth limit and vanilla maximum, `Mth.binarySearch`, data the game can't finish, D1 anchors.
- [ ] `world-saves.md`: modified UTF-8 handling, region length rule, unreadable files.
- [ ] `structure-placement.md`: empty alias lists.
- [ ] `locate.md`: radius and region caps.
- [ ] `search.md` and `cli-json.md`: confirming progress, partial outcomes, stop granularity, `other` feature kind.
- [ ] `oracle.md`, `data-sources.md`, getting-started, CLAUDE.md, README, architecture.md, `development-process.md`.
- [ ] CONTRIBUTING.md and SECURITY.md.

## Notes

Decisions, surprises and dead ends, newest first.

- 2026-10-28: Phase 3.
  - **Two levels of stop:** stopping had to ask for two things. Ctrl-C has always meant "stop and print the best so far", and printing needs the evaluations, so the first stop can't skip them. `SearchStop::stop` ends the search; `stop_now`, a second Ctrl-C or Skip evaluating, ends the evaluation too. Evaluation is now parallel, so the second is rarely needed.
  - **Exit criterion,** measured with a release build on the real 26.3 bundle (`require minecraft:stronghold within 1200 of spawn`, `--time 5s`):

    | Run             | Search | Wall time | Hits evaluated |
    | --------------- | ------ | --------- | -------------- |
    | `--top 10`      | 5.01 s | 5.61 s    | 10             |
    | `--top 1000`    | 5.02 s | 5.98 s    | 316            |
    | confirmed, 10   | 5.01 s | 5.43 s    | 10             |
    | confirmed, 1000 | 5.01 s | 6.06 s    | 322            |

    Wall time includes loading the bundle. Ctrl-C and the viewer's Stop set the same handle the time limit's cut checks, and the runner's tests cover them.

  - **A bug found while testing R5:** a region's column count multiplied two `i64`s that can each reach 2³², which overflowed. The same multiplication was in the CLI's own check. Both now saturate.
  - **What R9 didn't do:** a replaced locate isn't cancelled. The library's locate takes no stop handle, and adding one is API work for a case R4's caps already bound to minutes. Its result is dropped when it arrives.
  - The plan's `Confirming` progress phase wasn't needed: parallel evaluation of even 1,000 hits took under a second here.

- 2026-10-28: H3 to H11.
  - H4 turned up two more differences from `RegionFile`. An external chunk whose length is 0 is missing in the game, even when its `.mcc` file exists. Apart from that, the game ignores an external chunk's length, where spyglass had checked it against the sectors.
  - Reading each chunk by seeking made the region-file size cap unnecessary, so it's gone: the game reads oversized files, and now spyglass does too.
  - H7: the loader had refused zero weights, which the game's `Weighted` codec (`NON_NEGATIVE_INT`) accepts. A pack with one would have failed to load in spyglass only. `pick`'s `i32` sum could also overflow in a debug build; the load-time sum check rules that out.
  - H10 closed without a code change:
    - `find_closest_biome_3d` and `find_biome_horizontal` are only called with the game's constants: `/locate`'s steps and a structure's fixed increment.
    - `biomes_in` is only called from tests. No user input reaches any of them; the front ends go through the `spyglass` API.
    - Panicking on a violated, documented precondition is the Rust convention, as with `chunks(0)`. A `Result` or step newtype would be API churn that guards nothing.
- 2026-10-28: H2.
  - Measured on every bundle, vanilla density functions nest at most 48 deep (26.3, 26.4-snapshot-5 and -6) or 53 (26.2, whose references are nodes).
  - In a debug build, loading a chain overflowed a 2 MiB stack at about 180 levels, roughly 23 KB of stack per reference. So the limit is 128, not something larger: the deepest graph allowed loads, binds, samples and drops on a 2 MiB thread in debug.
  - A function loaded once keeps its height, so a reference to it from the cache counts in full. Without that, a pack could build depth up across separate loads, for example from successive router entries.
  - A debug build on a Windows main thread (1 MiB) could still overflow at the limit. Release frames are far smaller. Running the CLI's work on a thread with a larger stack would close this; it's left for R-phase work if it's wanted.
- 2026-10-28: H1 and H6.
  - Fixing the reservation alone wasn't enough. A well-formed 64 MiB list of bytes becomes 64 Mi `Tag`s, about 2 GiB. So `read_compressed_limited` also counts the tree's memory against its limit.
  - The test that a list's claimed length isn't reserved didn't abort under the old code on this machine: Windows accepted about 64 GB of reservations that were never touched. The test still guards the limited path, which now fails with `Memory` before the data runs out.
  - The game's `NbtAccounter.pushDepth` (26.3 bytecode) counts only lists and compounds, and throws once 512 are open. spyglass counted every tag and allowed an empty 513th container. It now matches the game.
  - At that depth the old reader overflowed a 2 MiB test thread in a debug build, which the game's limit allows. Containers now recurse through small functions of their own, and the deepest read fits in 1 MiB.
  - Strings decode as `readUTF` decodes them: a raw zero byte is accepted. An unpaired surrogate is U+FFFD in a value and an error in a name.
- 2026-10-28: Phase 1 done.
  - `cargo xtask links` found exactly the audit's six broken links (with the templates excluded) and nothing else. The `#column-evaluation` proof now lives under _Remembered values_.
  - Templates are excluded by file name (`template.md`), not by path, because the `no-process-doc-refs` hook rightly refuses a path naming the plans folder in xtask.
  - The mangled literals were all `\n` turned into real newlines, plus two `\` continuations whose `\` and newline were lost. Only the CLI's two messages and the `server.properties` line changed output.
  - SECURITY.md points reporters at GitHub's private vulnerability reporting, which has to be enabled in the repository's settings once it's on GitHub.
  - The X6 test moves to M10: the uncertain-candidates explanation is a CLI literal today, and M10 gives it a home in the library.
- 2026-10-28: Plan drafted from the audit. Choices still open for the user before phase 4:
  - The EULA opt-in mechanism: flag and environment variable, as recommended, or documentation only.
  - Whether fuzzing justifies a nightly toolchain, or the property-test fallback is enough.
  - Whether a Windows CI job is worth its runner minutes.
- 2026-10-28: D8 closed with no change. ADRs 0031 and 0032 mention the m09 plan in prose, not as links, and ADRs are never rewritten (ADR 0001).
