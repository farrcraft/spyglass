# m05. Cleanup before the viewer

- Status: Completed
- Milestone: [05. Viewer MVP](../../roadmap/completed/05-viewer-mvp.md)
- Started: 2026-10-25
- Completed: 2026-10-25

## Goal

Pay down the debt found in the repo assessment of 2026-10-25 before the viewer and seed search build on it. The viewer needs a public API that is cheap to call many times per frame and safe to share across threads. Seed search needs per-seed cost kept low. Neither should inherit the extractor's network stack, unbounded reads of untrusted files, or tests that only run when someone remembers.

This plan covers no exit criteria of milestone 05 directly. It makes "panning and zooming stay responsive" and "overlays match the public API exactly" achievable without first changing the API. It is the first plan for the milestone; the viewer plan follows it.

## Findings

From the assessment. Each one has a task below.

- **Architecture**
  - A1. `spyglass-data` depends unconditionally on `ureq`, `zip`, `clap`, `anyhow`, `tempfile` and `directories`, so every library crate and the public API pull in an HTTP client and a CLI parser. `BundleError::Zip` leaks a `zip` type downstream.
  - A2. Codec helpers are scattered. `LoadError` lives in `spyglass-density` but biome and structure use it. `full_id` exists three times, and `At`/field helpers twice. The `LoadError { resource, path, kind: Box::new(..) }` construction appears five times. `NOISE_SETTINGS` and registry enumeration are each written twice.
  - A3. Each `Session::locate_structure` call re-reads and re-parses tag JSON from disk (`World::targets` → `StructureSets::resolve` → `Bundle::json`) and checks existence twice.
  - A4. `World::session` clones the `Biomes` into `Placements` and binds the spawn noises up front even when nothing asks about spawn. Spawn and climate bind the same noises separately, so each seed builds them twice. Nothing asserts that `World` and `Session` are `Send + Sync`.
  - A5. The public `Error` exposes `BundleError`, `spyglass_density::LoadError` and `WorldError` as payloads, and some variants are misused:
    - `Unknown { what: "world save" }`
    - an ambiguous save name reported as `Unsupported`
    - region-scan I/O errors reported as `Bundle(Io)`
  - A6. Library policy lives in the CLI: bundle selection (newest, or `--game-version` matched against a directory name rather than `BundleInfo.version`) and the nearest-first slime ordering that `cli-json.md` specifies.
  - A7. The density tree evaluator evaluates shared subtrees (shifts, continents, erosion, ridges) once per reference. `at_column` builds a new tree per column, about 160k times in one `/locate biome`, and splits shared nodes apart.
- **Correctness and robustness**
  - C1. `Binder::bound` is keyed on `*const DensityFunction` without holding the `Arc`, so after a function is dropped a new one at the same address gets the old sampler. `Binder` is public.
  - C2. Untrusted saves have no size limits.
    - `nbt::read_compressed` and region chunk decoding call `read_to_end` without a cap.
    - `.mcc` files are read whole.
    - A chunk's length isn't checked against its sector count.
  - C3. `packs.rs` recurses with `path.is_dir()`, which follows symlinks, so a symlink cycle loops forever. `file/<name>` pack names from `level.dat` are joined without rejecting `..` (`packs.rs`, `Save::file_packs`).
  - C4. Bad inputs can hang or panic.
    - `find_biome_horizontal` loops forever when `increment <= 0` and uses non-wrapping `+=`.
    - `NormalNoise::new` panics on bad parameters (out-of-range `exp2i`, short `amplitude_modifiers`) because only the density loader validates them.
    - `sampler.rs` maps every `NoiseError` to "legacy".
  - C5. `check.rs` and `jigsaw.rs` use plain `+`/`-` on block coordinates where the rest of the code wraps, which panics in debug builds at extreme chunk coordinates.
  - C6. `placement.rs` calls `dedup()` on chunks that may not be sorted, so non-adjacent duplicates survive.
  - C7. `mth::floor` documents `(int) Math.floor(d)`, but the classic `Mth.floor` (`(int) d`, minus one if `d < i`) gives `i32::MAX` for -1e20. Check the 26.3 bytecode.
  - C8. Any `?` between `spawn()` and `wait()` in `xtask oracle locate` drops a running server without killing it, and `messages[i]` can panic on a bad marker.
  - C9. `router.rs` has a comment about naming inline router functions, but the code rejects inline entries, which the game's codec accepts. `density-functions.md` doesn't say so.
  - C10. CLI: `json!` on `PathBuf` panics on non-UTF-8 paths, and `print_json` silently drops non-object fields.
- **Tests**
  - T1. The oracle and golden tests (23, all `#[ignore]`d) never run from the hooks or `cargo xtask check`.
  - T2. Without a bundle, structure locate is barely tested: `check::select`, the locate loop, `MAX_UNCERTAIN`, `certain_answer`, `ConcentricRings::positions`, `Session::structures_in` and `Session::biome_at`. Nothing covers `inspect_save` warnings that need no bundle.
  - T3. Further gaps:
    - `find_biome_horizontal` (reservoir sampling and tie replay)
    - `find_closest_biome_3d` itself
    - `Gaussian` and `LegacyPositionalRandomFactory`
    - `Save::packs()`
    - region edge cases: short file, offset past the end, length 0, missing `.mcc`
    - an NBT "never panics on arbitrary bytes" test
- **Docs**
  - D1. The README says "scaffolding only. Nothing works yet."
  - D2. The `repository` field in `Cargo.toml` is still `OWNER`.
  - D3. `docs/README.md` describes specs as three topics, but there are now twelve.
  - D4. Every spec says Draft, so the status column carries no information.
  - D5. `spyglass-search` and `spyglass-terrain` are one-line stubs, which contradicts architecture.md. The README crate table lists them as if they exist and omits `spyglass-oracle`.
  - D6. In architecture.md, "Dependencies only point down this table" reads backwards.
  - D7. getting-started §3 runs the placeholder viewer.
  - D8. `cli-json.md`'s status line says it covers only `locate`, but it also documents `world`.
  - D9. `Session::biome_at` doesn't say it returns the stored quart biome, not the zoomed one the F3 screen shows.
  - D10. `char::is_numeric` doc says "non-ASCII digits". The crate docs say "jigsaw assembly", but only the start piece is implemented.
- **Idiom and organization**
  - I1. Some public items exist only for tests:
    - `java_round`, `round_trig_product`, `MAX_UNCERTAIN` and jigsaw internals re-exported from `spyglass-structure`
    - `BiomeSource::tree`, `SearchTree::brute_force` and `ClimateSampler::function`
    - `Save::file_packs`
    - `Session::structures_in`, which is unused
  - I2. Some functions are too long:
    - `find_biome_horizontal` (~90 lines; `allow(too_many_arguments)` without `reason =`, also on `find_closest_biome_3d`)
    - xtask's `doctor`, `server` (locate), `linux_check` and `oracle` runner
  - I3. xtask duplicates the extractor's resolve steps (manifest → version → server jar → Java major), `javac.exe` and `sha1_hex`; `io_err` exists three times.
  - I4. `cli/main.rs` builds JSON ad hoc with `json!`, and its 684 lines mix arguments, locate and world commands.
  - I5. Smaller items:
    - `Spline::Multipoint` is three parallel public `Vec`s with no invariant.
    - The `Sampler.y` bool sits next to `Node::Noise { y }`, a scale.
    - `sampler.rs` nests a `let-else` with `unreachable!()`.
    - `router.rs` builds `Vec` → `try_into().expect`.
    - `spawn.rs` re-evaluates a sample it already has.
    - `biomes_in` doesn't check for size overflow.
    - `check.rs` has an `.expect` on jigsaw settings that a `StructureType::Jigsaw(Jigsaw)` would rule out.
    - `structure_template` scans the whole manifest on every call.
    - `generator_warning` reopens the bundle for every save.
    - `bundles_in(..).unwrap_or_default()` hides why no bundle was found.
    - `region.rs` re-panics a reader thread's panic.
    - `spyglass_world::to_json` is a codec helper.
  - I6. Counting chunk versions fully parses every chunk, and the NBT reader builds a path string for every element even when parsing succeeds.
  - I7. Smaller items in data and setup:
    - `cache.rs` builds a filename from a remote SHA-1 without validating it.
    - `cache.rs` has a `u64::MAX` body limit.
    - `bundler.rs` doesn't say why `Entry::sha256` is unchecked.
    - The winget path in `setup.ps1` doesn't pass `--default-toolchain none`.

## Approach

Six phases, each a commit or two that passes `cargo xtask check` and the oracle tests on its own. The order puts quick, low-risk fixes first and saves the API changes for when the crate split has settled the types they depend on.

1. **Docs and hygiene** (D1–D10, I7 setup). Text only, plus removing the two stub crates.
2. **Bugs and hardening** (C1–C10, I6). Local fixes, each with a test that fails before it. No API changes except `NormalNoise::new` returning new `NoiseError` variants.
3. **Crate split and codec** (A1, A2, I3). One new crate and an ADR; code moves, behavior stays the same.
   - **New `spyglass-extract` crate:** `manifest`, `cache`, `java`, `datagen`, `bundler`, `extract`, the writing half of `bundle` (staging, `copy_jar`, the `Exists` and `Zip` errors as `ExtractError`), and the `spyglass-extract` binary. It owns `ureq`, `zip`, `clap`, `anyhow`, `tempfile` and `directories`. xtask depends on it.
   - **Slimmer `spyglass-data`:** keeps reading bundles (`Bundle`, `read_manifest`, `verify`), `codec` and `nbt`, with `serde`, `serde_json`, `thiserror`, `flate2` and `sha1`.
   - **`spyglass_data::codec` takes:** `LoadError` (with a constructor), `At`, the field helpers, `full_id` and `registry_ids`. `NOISE_SETTINGS` is defined once, in the lowest crate that uses it.
   - `extract::resolve(version, cache)` serves both the extractor and `xtask oracle`.
   - The split is chosen over feature flags because `--workspace` builds unify features, so the network stack would still be compiled and linked in tests.
4. **Public API** (A3–A6). Breaking changes inside the workspace only (nothing is published).
   - **Handles:** `World::structure_query(&str) -> Result<StructureQuery>` and `World::biome_id(&str) -> Result<BiomeId>` resolve once. The `Session` methods take the handle; string-taking conveniences call the resolver.
   - **Binding a seed:** `Placements` borrows the `Biomes` instead of cloning them. Spawn becomes a `OnceLock`, like the rings. Climate and spawn share one noise binding per seed.
   - **Thread safety:** a compile-time `Send + Sync` assertion for `World` and `Session`.
   - **`Error`:**
     - It becomes `#[non_exhaustive]`.
     - New variants: `Io { path, source }` and `AmbiguousSave { name, matches }`.
     - `Bundle`, `Load` and `Save` become opaque `Data(DataError)`/`Save(SaveError)` wrappers that expose `source()`, not the lower types.
     - `Unknown` keeps only structure, tag and biome.
   - **Out of the CLI:** `spyglass::find_bundle(dir, Option<&str>)` matches `BundleInfo.version`, and `Session::slime_chunks_near(from, radius)` returns chunks nearest first.
   - If the handles change ADR 0014's decision (a `World` loaded once, seed-bound `Session`s), a new ADR supersedes it. If they only refine its API, the specs record them and no ADR is needed.
5. **Density performance** (A7). Same results, less work.
   - Number the bound nodes when binding: `Binder` already de-duplicates by identity, so each shared node gets a slot index.
   - Evaluate a point with a small per-call slot cache, so each shared node runs once per position.
   - Replace `at_column`'s per-column tree with a column context that pins `x`/`z` in the slot cache.
   - This is a step toward the flattened evaluator in the architecture doc, not that evaluator itself.
   - Benchmark before and after with `/locate biome` and a 256×256 quart region fill.
6. **Tests, idiom and the oracle runner** (T1–T3, I1–I5, I7).
   - **`cargo xtask oracle-test`:** runs the ignored tests in release. It reports clearly which ones lack their `oracle-out/` or `bundles/` inputs rather than failing silently. The development process makes it a step in completing any plan that touches worldgen.
   - **Unit tests** reuse the synthetic bundle from `structure/load.rs`.
   - **CLI split:** typed `Serialize` output structs, and `main.rs` broken into `args.rs`, `locate.rs` and `world.rs`.

## Tasks

### 1. Docs and hygiene

- [x] README:
  - replace the status line with what works today (locate on seeds and saves, CLI with JSON)
  - mark planned crates as planned
  - add `spyglass-oracle` to the crate table
  - match the crate order in architecture.md
- [x] `Cargo.toml` `repository`: dropped until the repo is on GitHub.
- [x] Delete `crates/spyglass-search` and `crates/spyglass-terrain`, and remove them from docs that list them as existing. Keep them in architecture.md as planned, in a "Planned" column or note.
- [x] architecture.md: reword the dependency rule as "a crate may depend only on crates listed above it". Update the data crate's role for the split (phase 3).
- [x] `docs/README.md`: specs describe "behavior the code must match", without a stale topic list.
- [x] `docs/specs/README.md`: give each spec a real status. Use `Stable` (implemented and checked against the oracle) or `Draft`, define the two in the README, and set each spec accordingly.
- [x] getting-started §3: drop `cargo run -p spyglass-viewer` until the viewer exists, or label it a placeholder.
- [x] `cli-json.md`: fix the status line to cover `world`.
- [x] Doc comments: `Session::biome_at` (quart biome, not the zoomed one); `char::is_numeric` (any Unicode numeric, refused, which is safe); `spyglass-structure`'s description (start piece only).
- [x] `setup.ps1` winget path: no change needed (see notes).

### 2. Bugs and hardening

- [x] C1 `Binder`: each cache entry keeps its `Df` alive. Test: bind, drop and rebind 64 functions (it failed before the fix).
- [x] C2 Limits on reads from saves:
  - `nbt::read_compressed_limited`, used for every NBT file in a save, with 64 MiB on disk and decompressed
  - region files capped at the format's maximum size; a chunk's length must fit its sectors (as `RegionFile` checks); chunks and `.mcc` files capped at 32 MiB stored and decompressed
  - tests: an NBT bomb, and a region file with a short header, a length past its sectors, length 0, a missing `.mcc`, an offset past the end and a zlib bomb (it failed before the fix)
- [x] C3 Packs: walked by `entry.file_type()`; a `file/` name that isn't a single file name is `Missing` and never read. Tests for both (the symlink cycle on Unix only). `Save::file_packs` removed (I1).
- [x] C4 Bad input:
  - `find_biome_horizontal` asserts a positive increment (as `find_closest_biome_3d` does for its steps) and its loops wrap as the Java's do
  - `NormalNoise::new` checks the codec's ranges itself (`NoiseError::BaseOctave`, `OctaveCount`, `AmplitudeModifiers`), and `sampler.rs` passes each through
- [x] C5 `check.rs`, `jigsaw.rs` and `placement.rs` (exclusion zones) wrap block and chunk arithmetic. The test at chunk ±134217727 waits for the synthetic-bundle tests in phase 6.
- [x] C6 `structure_chunks_in`: repeated ring positions are dropped by a set, keeping order. Random-spread cells can't repeat.
- [x] C7 `Mth.floor`: no change needed (see notes).
- [x] C8 `xtask oracle locate`: `KillOnDrop` around the server, `get_mut` for markers, and `server` split into `server_dir`, `Server::start`/`next` and the run.
- [x] C9 Inline router entries: supported through `Loader::inline`, with a test, and in `density-functions.md`.
- [x] C10 CLI paths are lossy strings in JSON (in `cli-json.md`), and `print_json` refuses non-object fields. The typed-output task replaces both.
- [x] I6 Chunk versions read with `nbt::read_root_int`, and the NBT reader keeps a path stack and renders it only for errors.

### 3. Crate split and codec

- [x] ADR: the `spyglass-extract` crate, and the read-only `spyglass-data` ([ADR 0016](../../adr/0016-extractor-crate-and-read-only-data.md)).
- [x] `crates/spyglass-extract`: manifest, cache, java, datagen, bundler, extract, `write` (the bundle-writing half of `bundle`, with `WriteError`) and the binary. `cargo run -p spyglass-extract` in docs and `xtask linux-check`.
- [x] `spyglass-data` depends on `flate2`, `serde`, `serde_json`, `sha1` and `thiserror` only. `cargo tree -p spyglass -e normal` has no `ureq`, `clap`, `anyhow`, `tempfile` or TLS; `zip` and `directories` remain, from `spyglass-world`.
- [x] `BundleError` keeps reading errors only; `Zip` and `Exists` are `WriteError`'s.
- [x] `spyglass_data::codec`: `LoadError` (with `new`), `LoadErrorKind`, `LoadResult`, `At`, `field`, `object`, `id`, `float`, `double`, `int`, `int_in`, `int_field`, `out_of_range`, `full_id`; `Bundle::registry_ids`. Every copy deleted; `spyglass-structure` no longer depends on `spyglass-density`.
- [x] `NoiseError::reason` is the one message source; the density loader maps through it.
- [x] `NOISE_SETTINGS` is `spyglass_density::NOISE_SETTINGS`; `Resources` is implemented for `HashMap<String, Value>`, the one test double.
- [x] `extract::resolve` serves the extractor and `xtask oracle`; `java::JAVAC_EXE` is public; one `bundle::sha1_hex` for bytes and files. The three `io_err` helpers stay: each builds its own module's error type.
- [x] I7 data: the cache refuses a SHA-1 that isn't 40 hex digits (`CacheError::BadSha1`) and stops a download one byte past its size; `Entry::sha256` says why it isn't checked.
- [x] Docs: README, architecture.md, getting-started, `bundle-format.md`, `data-sources.md`.

### 4. Public API

- [x] `World::structure_query` and `World::biome_query` resolve a name once into a `StructureQuery` or `BiomeQuery`; `locate_structure`, `structures_in` and `locate_biome` take them. A query used with another world's session is `Error::OtherWorld`. No string conveniences: one path, and resolving is a single call.
- [x] Binding a seed: measured at 0.1 ms per session (climate and spawn together), so `Placements` keeps its clone of `Biomes` (six `Arc`s), spawn stays eager and the two noise bindings stay separate. Not worth the complexity.
- [x] Before/after numbers in the notes.
- [x] A compile-time `Send + Sync` check for `World`, `Session`, the queries and `Error`.
- [x] `Error` is `#[non_exhaustive]`, with `Io`, `NoBundle`, `AmbiguousSave` and `OtherWorld`; bundle, load and save failures are `Data(Cause)`/`Save(Cause)`, an opaque `Cause`. `save.rs` uses `AmbiguousSave` and `Io`; `Unknown`'s doc includes world saves. The CLI's tests pass unchanged.
- [x] `spyglass::find_bundle(dir, version)`: newest, or the one whose `BundleInfo.version` matches. The CLI uses it, so `--game-version` matches the bundle's version, not its folder name.
- [x] `Session::slime_chunks_near(centre, radius)`, used by the CLI.
- [x] `Session::structures_in`: kept (the viewer needs it), and tested on the fixture world (phase 6).
- [x] `SaveInspector` reads each bundle's world presets once, for `world list`; `inspect_save` is one inspector, one save. A missing `bundles/` has no bundles, and each save's `NoBundle` warning says to extract one.
- [x] ADR 0014's decision stands (handles refine its methods); `locate.md` describes the queries.
- [x] `cargo xtask bench` (`crates/spyglass/examples/bench.rs`), brought forward from phase 5 to measure this phase.

### 5. Density performance

- [x] Benchmarks: `cargo xtask bench` (phase 4), covering `biome_at` over 256 × 256 quarts, `/locate biome` at radius 6400, structure locates and spawn.
- [x] Every bound node but constants gets a slot from its `Binder`; a per-thread memo stamped with the binder, the column and the y remembers values, so shared nodes run once per position and y-independent ones once per column.
- [x] `Sampler::at_column` and `ClimateSampler::at_column` are gone: column-by-column callers get the same saving from the memo, without rebuilding trees or splitting shared nodes.
- [x] The oracle tests pass unchanged; numbers in the notes. I5's `Spline::Multipoint` now holds a validated `Points`, and the sampler's renames (`depends_on_y`, `y_scale`, `xz_scale`) and `match` came with the rewrite.

### 6. Tests, idiom and the oracle runner

- [x] `cargo xtask oracle-test` runs, in release, every ignored comparison whose inputs exist, and names each one it skips and the command that makes its inputs. In getting-started, the oracle spec, CLAUDE.md, and step 4 of `development-process.md`.
- [x] `spyglass_oracle::fixtures`: a hand-written Overworld bundle (plains west of x = 0, desert east, optionally forest tied with plains) that the public API opens. The structure crate's own test bundles use its writer too.
- [x] API tests on it (`crates/spyglass/tests/synthetic.rs`):
  - biome lookup
  - a certain answer ending the search, and the same chunk in `structures_in`
  - candidates stopping at `MAX_UNCERTAIN`
  - tags expanding in order
  - concentric rings
  - biome locates
  - queries tied to their world
  - coordinates at the edge of the world (fails on the code before C5)
  - slime ordering
  - spawn and `find_bundle`
- [x] `inspect_save` on saves built in the test: no warnings, every non-blocking warning, another generator, and no bundles folder (`crates/spyglass/tests/save_warnings.rs`).
- [x] Biome searches on the fixture: closest and random horizontal matches, tie replay at and over the limit, the 3D search's certain and uncertain hits, and region fills (`crates/spyglass-biome/tests/search.rs`).
- [x] RNG: Gaussians against Java (see notes) and the legacy positional factory against the spec's `getSeed`.
- [x] World: `Save::packs()` and the region edge cases (phase 2).
- [x] NBT: every truncation and nine damaging values at every byte of a valid file return errors or values, never panic.
- [x] I1: `java_round` and `round_trig_product` are crate-private; the jigsaw internals aren't re-exported; `BiomeSource::tree` and `SearchTree::brute_force` are `#[doc(hidden)]` for the golden test; the unused `ClimateSampler::function` is gone; `Save::file_packs` went in phase 2.
- [x] I2:
  - `find_biome_horizontal` takes a `HorizontalArea` and is split into `scan_cells`, `choose` and `replay_ties`
  - `find_closest_biome_3d` takes a `SearchVolume`
  - xtask's `doctor`, `linux-check` and `oracle golden` are split into their steps
- [x] I4: typed `Serialize` output (`json.rs`), and `main.rs` split into `args.rs`, `locate.rs`, `world.rs` and `json.rs`. Fifteen `--json` and text outputs, from seeds and saves, are byte-identical before and after.
- [x] I5: all but one, which was declined (see decisions).
  - the spline `Points` type, the renames and the `match` (phase 5)
  - router arrays built directly
  - spawn's duplicate sample skipped
  - `biomes_in`'s size checked
  - `Bundle` knows whether it has templates
  - a panicking region reader is an I/O error
  - `Tag::to_json` in `spyglass_data::nbt`

## Verification

Done on 2026-10-25:

- `cargo xtask check` passes after each phase.
- `cargo xtask oracle-test`: all 20 comparisons pass after each phase that touched worldgen, and the oracle suite now takes under 2 minutes instead of about 6.
- `cargo xtask oracle golden` regenerates the golden set through the restructured command.
- `cargo xtask oracle locate` runs against the real server and the game's answers still agree (phase 2).
- All 191 saves on this machine read. One 1.12 save has 85 unreadable chunks, as it did before the limits.
- A fresh `spyglass-extract 26.3` writes a byte-identical `bundle.json`.
- `cargo tree -p spyglass -e normal` has no `ureq`, `clap`, `anyhow`, `tempfile` or TLS crates.
- The CLI's JSON and text output is byte-identical across the refactor.
- Bugs fixed with a test that fails before the fix: C1 (`Binder`), C2 (decompression bomb), C5 (edge coordinates).

## Durable outputs

- [x] ADR: `spyglass-extract` split and the read-only `spyglass-data` ([ADR 0016](../../adr/0016-extractor-crate-and-read-only-data.md))
- [x] ADR superseding 0014: not needed; the handles refine its methods and `locate.md` describes them
- [x] ADR for the benchmark harness: not needed; it adds no dependency
- [x] `docs/architecture.md`: crate table, planned crates, dependency wording, `spyglass-extract`, the oracle crate's fixtures
- [x] `docs/specs/README.md`: status definitions and real statuses
- [x] `docs/specs/oracle.md` and getting-started: `cargo xtask oracle-test`, `cargo xtask bench`
- [x] `docs/development-process.md`: run the checks and the oracle tests when completing a plan
- [x] `docs/specs/density-functions.md`: inline router entries; remembered values instead of column folding
- [x] `docs/specs/noise.md`: `Mth.floor` needed no change (the code matched the bytecode)
- [x] `docs/specs/rng.md`: `java.util.Random` Gaussians differ from the game's in the last bit
- [x] `docs/specs/world-saves.md`: size limits and refused pack names
- [x] `docs/specs/bundle-format.md`, `data-sources.md`: the extractor's new crate; the limited and shallow NBT reads
- [x] `docs/specs/cli-json.md`: status line; paths
- [x] `docs/specs/locate.md`, `biome-lookup.md`: queries; region fills
- [x] README, `docs/README.md`, getting-started, CLAUDE.md

## Decisions

Agreed on 2026-10-25, as recommended:

- **Stub crates:** `spyglass-search` and `spyglass-terrain` are deleted. They're recreated in the milestones that need them.
- **`repository` in `Cargo.toml`:** dropped until the repo is on GitHub.
- **Benchmarks:** a timed `cargo xtask bench`, with no new dependency.
- **Phase 5:** in this plan.
- **`StructureType::Jigsaw(Jigsaw)`** (I5): declined. `StructureType` is a `Copy` key matched throughout the crate, and the loader is the only code that pairs a jigsaw kind with its settings, so the `expect` can't fire. Moving the settings into the enum would cost more than it buys.
- **Shared `io_err`** (I3): declined. Each of the three builds its own module's error type.

## Notes

- 2026-10-25: Completed. Milestone 05 continues with the viewer plan.
- 2026-10-25: Phase 6 done.
  - The fixture world's temperature gradient first crossed exactly 0 at x = 0, tying a whole column of cells between plains and desert. That's correct behavior (the ring search reported its alternatives as unknown), but not what the tests wanted, so the gradient now ends at 1001.
  - `java.util.Random(42).nextGaussian()` and spyglass differ in the last bit of the first pair. `java.util.Random` uses `StrictMath.log`, the game's `MarsagliaPolarGaussian` uses `Math.log`, and spyglass matches `Math.log` here. The test accepts either result for that pair, since Rust's `ln` is platform-dependent too; `rng.md` records it.
- 2026-10-25: Phase 5 done. `cargo xtask bench`, same session, before → after (ms): village locate 363 → 51; stronghold locate in a new session 2,437 → 957; cherry grove locate 71 → 47; 65,536 `biome_at` 423 → 158; spawn origin 3.8 → 1.2. The ignored oracle suite went from about 6 minutes to under 2.
- 2026-10-25: Phase 4 done. `cargo xtask bench`, medians in ms, before → after:
  - `World::open` 334 → 330; `World::session` 0.10 → 0.10
  - village locate 459, or 505 when resolved from text → 363 resolved, 385 from text (the text path re-parses tags; the runs are noisy by about 20%)
  - stronghold locate in a new session 3,175 → 2,437; cherry grove locate 102 → 71; 65,536 `biome_at` 422 → 423. Nothing on these paths changed, so the differences are run-to-run noise: compare only within one run, or repeat.
  - Everything that's slow is density evaluation, which phase 5 targets.
- 2026-10-25: Phase 3 done. A fresh `spyglass-extract 26.3` writes a `bundle.json` byte-identical to the existing bundle's.
- 2026-10-25: Phase 2 done.
  - The 26.3 bytecode of `Mth.floor` is `Math.floor` then `d2i`, which is exactly `(int) Math.floor(d)`, so `mth::floor`, its doc and its test were right. The classic `i - 1` form the assessment had in mind isn't the current one.
  - `world info` is about 2× faster on hermitcraft9 (1.18-era chunks, 2.3 GB), at about 3.7 s against 6.6 to 9.9 s, with byte-identical JSON. The largest save (4.7 GB, older chunks) is unchanged at about 8 s, so I/O and decompression bound it.
  - `zip` stays in `spyglass-world`, which reads zipped data packs, so phase 3's dependency check expects it there, read-only.
- 2026-10-25: Phase 1 done.
  - The rustup that winget installs makes `stable` the default toolchain. `rust-toolchain.toml` pins `stable` too, so the two setup scripts end up in the same state and `setup.ps1` needs no change.
  - Every spec is now marked Stable: each one's behavior is implemented and checked. The index gained a "Covers" column so its statuses carry information.
- 2026-10-25: Started, with the open questions decided as recommended.
- 2026-10-25: Drafted from the repo assessment: three reviewers over the crates, plus a check of the docs, build and tests. At the time: clippy clean; 116 tests pass; the 23 ignored oracle and golden tests pass in about 6 minutes (release); no forbidden references or committed game data. The data crate's dependencies, the `Binder` key, the uncapped decompression and the per-call `fs::read` were confirmed by reading the source. `mth::floor` is unconfirmed until the bytecode is checked (C7).
