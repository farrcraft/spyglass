# m11. Releases

- Status: In progress
- Milestone: [11. Releases](../roadmap/11-releases.md)
- Started: 2026-10-28
- Completed: YYYY-MM-DD

## Goal

Versioned release archives, built by CI from a tag, for people who don't build from source. Covers every exit criterion of milestone 11.

## Approach

### Versions and tags

- One SemVer version for the whole workspace, in `[workspace.package]`, starting at `0.1.0`. Crates stay `publish = false`. The version describes the binaries and the public API together. While it's `0.x`, a minor bump may break the API, the CLI's flags or its `--json` output.
- Tags are `vX.Y.Z` on `main`. The tag must equal the workspace version, and the release workflow checks it.
- Adding a game version is a minor release, not a patch. It changes what the binaries accept.
- `CHANGELOG.md` at the root, in Keep a Changelog form, with an `Unreleased` section that changes add to. A release's notes are its section, copied by the workflow. Each section lists the game versions the release supports.

### Binaries outside the repository

Today `--bundles` and the extractor's `--out` default to `bundles`, relative to the working directory. That only makes sense in a clone. Library first: the default comes from the API, not the front ends.

- `spyglass_data::default_bundles_dir()`, re-exported by `spyglass`, picks in this order:
  1. `SPYGLASS_BUNDLES`;
  2. `./bundles`, if it exists, so a clone works as it does now;
  3. the per-user data folder (`directories::ProjectDirs::data_dir()/bundles`).
- The CLI, extractor and viewer use it when no folder is given. Their help text shows the folder chosen.
- The rules go in [bundle-format.md](../specs/bundle-format.md). The extractor's jar cache already uses `ProjectDirs` (`crates/spyglass-extract/src/cache.rs`), so no new dependency is needed.

### Which game versions a build knows

Switch ranges are open-ended ([ADR 0026](../adr/0026-how-game-versions-coexist.md)). So a binary from today, given a bundle for a version released next year, would silently answer with today's behavior. A release has to say what it was checked against.

- `spyglass_data::versions::CHECKED`: the data versions in the CI oracle matrix, with their ids. A test keeps it equal to the matrix in `.github/workflows/ci.yml`.
- Loading a bundle whose data version isn't in `CHECKED` succeeds with a warning: an error in the API's result type, a stderr line in the CLI and `"unchecked_version"` in `--json`, and a banner in the viewer. "Newer than this build" and "between checked versions" get different wording.
- The extractor warns too, before downloading.
- `spyglass --version` lists the checked versions after the build line.

### Version strings

- The three binaries print `<name> <version> (<commit> <date>)`. The CLI and extractor print it from `--version`; the viewer shows it in its side panel.
- The commit and date come from `SPYGLASS_BUILD_COMMIT` and `SPYGLASS_BUILD_DATE`, read with `option_env!`. `cargo xtask dist` sets them; plain `cargo build` gives `(dev build)`. No `build.rs` and no git dependency in the crates.
- `SECURITY.md` and the bug report advice ask for this line in place of "the commit".

### `cargo xtask dist [--target <triple>]`

- Builds `spyglass`, `spyglass-viewer` and `spyglass-extract` with `--profile dist --locked` for one target (the host by default).
- Sets `SOURCE_DATE_EPOCH` from the commit date. Strips build machine paths with `--remap-path-prefix` for the workspace, `CARGO_HOME` and the rustup toolchain. Windows targets link the C runtime statically (`-C target-feature=+crt-static`), so no Visual C++ redistributable is needed.
- Writes `target/archives/spyglass-<version>-<target>.{zip|tar.gz}` (`target/dist/` is where the `dist` profile builds without `--target`) with a deterministic file order, fixed timestamps and fixed permissions. Contents:
  - the three binaries;
  - `README.md`;
  - `LICENSE`;
  - `THIRD-PARTY-LICENSES.html`;
  - `GETTING-STARTED.txt`: Java, extracting a bundle, where bundles go, unsigned binaries on Windows and macOS.
- Checks every archive before writing it:
  - an allowlist of names, so nothing from `bundles/`, `oracle-out/` or a jar can get in;
  - no build machine path in any binary: a scan for the workspace, `CARGO_HOME` and home folder strings.
- Third-party notices come from `cargo-about`, pinned and installed by the setup scripts only with `--release-tools`, and by the release workflow. Its config allows the same licenses as `deny.toml`. A test keeps the two lists equal.
- `cargo xtask doctor` reports `cargo-about` and the release targets' rustup components as optional.

### `cargo xtask release <version>`

Local preparation, never pushes:

1. Checks that the tree is clean, the branch is `main`, and `Unreleased` has entries.
2. Sets the workspace version and refreshes `Cargo.lock`.
3. Turns `Unreleased` into `[<version>] - <date>` with the supported game versions, and opens a new `Unreleased`.
4. Runs `cargo xtask check`.
5. Commits as `Release <version>`, tags `v<version>` (annotated), and prints the `git push` commands.

### Release workflow (`.github/workflows/release.yml`)

On a pushed `v*` tag:

1. **verify:** the tag matches the workspace version and is on `main`, and `CHANGELOG.md` has its section.
2. **ci:** calls `ci.yml`, which gains a `workflow_call` trigger: every check and every oracle job, so a release is never built from a commit that failed any of them.
3. **build:** a matrix of targets, each running `cargo xtask dist --target <t>` and uploading the archive as a workflow artifact. It holds only spyglass's binaries, nothing game-derived, so ADR 0008 allows it:

   | Target                     | Runner         | Notes                                                                  |
   | -------------------------- | -------------- | ---------------------------------------------------------------------- |
   | `x86_64-pc-windows-msvc`   | `windows-2025` | Static CRT.                                                            |
   | `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | The oldest supported image, so the glibc requirement is 2.35 or older. |
   | `aarch64-apple-darwin`     | `macos-15`     |                                                                        |
   | `x86_64-apple-darwin`      | `macos-15`     | Cross-compiled; smoke-tested under Rosetta.                            |

4. **smoke:** on each target's runner, unpacks the archive into a clean folder outside the checkout, with no Rust on `PATH`, and runs `--version` and `--help` for all three. On Linux it also extracts 26.3 with the packaged extractor and runs one `locate` and one `render` against it. Nothing it extracts is uploaded.
5. **publish:** downloads the archives, writes `SHA256SUMS`, attests each archive with `actions/attest-build-provenance`, and creates a **draft** release with `gh release create --draft`, the changelog section as notes and the archives attached. Only this job gets `contents: write`, `id-token: write` and `attestations: write`; every other job keeps `contents: read`.

Every action is pinned by commit SHA, as `ci.yml` does. The workflow only calls `cargo xtask` and `gh`, so the steps run locally too.

### Decisions recorded

- **ADR 0037, versions and releases:**
  - one workspace version;
  - tags and the changelog;
  - a new game version is a minor release;
  - releases are built only from green CI;
  - drafts are published by hand.
- **ADR 0038, how release archives are built:**
  - `cargo xtask dist` instead of `cargo-dist`;
  - the targets;
  - static CRT;
  - path remapping and reproducibility;
  - `cargo-about`;
  - unsigned binaries with checksums and provenance, and when to revisit signing.
- **ADR 0039, where installed binaries find bundles,** and warnings for game versions a build wasn't checked against.

## Tasks

- [x] ADRs 0037, 0038 and 0039, accepted before the code that depends on them.
- [x] `default_bundles_dir()` in `spyglass-data`, re-exported. CLI, extractor and viewer use it. Tests for each step of the order, using `SPYGLASS_BUNDLES` and a temp folder. bundle-format.md updated.
- [x] `versions::CHECKED` and its sync test against `ci.yml`. The unchecked-version warning in the API, CLI (text and `--json`), viewer and extractor. Unit tests for the check, its wording and the JSON (see the notes).
- [x] Version strings with commit and date in all three binaries, and the checked versions in `spyglass --version`.
- [x] `CHANGELOG.md` with `Unreleased` filled in from milestones 01 to 10. The workspace version stays `0.0.0` until `cargo xtask release 0.1.0` sets it (see the notes).
- [x] `cargo xtask dist`: build, archive, allowlist, path scan. Unit tests for archive determinism (same inputs, same bytes) and the allowlist.
- [x] `cargo-about` config, its license list kept equal to `deny.toml` by a test, `THIRD-PARTY-LICENSES.html` in the archive. Setup scripts (`--release-tools` / `-ReleaseTools`, the pair kept in step) and `doctor`.
- [x] `GETTING-STARTED.txt` for the archive, written for someone with no Rust and no clone.
- [x] `cargo xtask release <version>`.
- [ ] `ci.yml` gains `workflow_call`. `release.yml` with verify, ci, build, smoke and publish. The release notes' relative links are made absolute to the tag (`https://github.com/farrcraft/spyglass/blob/vX.Y.Z/...`), since they don't resolve on the release page.
- [ ] A dry run: push a `v0.1.0-rc.1` tag to a fork, or run the workflow by `workflow_dispatch` with publishing off. Fix what it finds.
- [ ] A reproducibility check: build the same tag twice and compare hashes. Record the result in ADR 0038. Windows done (identical, see the notes); Linux and macOS in the workflow's dry run.
- [ ] Manual check on a clean Windows, macOS and Linux machine, by the first-run guide only.
- [ ] `docs/releasing.md`: cutting a release, what to do when a step fails, deleting a bad draft. Links from the README, CONTRIBUTING (changelog entries) and getting-started.
- [ ] README "Install" section pointing at the releases page, ahead of building from source.

## Verification

- Unit tests for the bundle folder order, the unchecked-version warning, the version strings, archive determinism, the allowlist and the license-list sync.
- `cargo xtask dist` on Windows locally, and `cargo xtask linux-check` extended to run it in its fresh Ubuntu container.
- The dry run of the release workflow, then `v0.1.0` itself.
- The exit criteria's clean-machine checks.

## Durable outputs

- [x] ADRs 0037, 0038, 0039.
- [x] [bundle-format.md](../specs/bundle-format.md): the default bundles folder.
- [x] A spec section on checked versions and the warning, in [bundle-format.md](../specs/bundle-format.md#checked-versions).
- [ ] `docs/releasing.md`.
- [ ] `CHANGELOG.md`.
- [ ] README, getting-started, CONTRIBUTING and SECURITY updated.

## Notes

- 2026-10-28: `cargo xtask release` takes pre-releases too (`0.1.0-rc.1`), for the workflow's dry run: they set the version and tag but leave the changelog alone, so their notes are Unreleased. The changelog gets Keep a Changelog's link definitions at the end, and markdownlint's MD024 is now `siblings_only`, since every release repeats headings like "Added" and "Game versions". The command refuses before changing anything if the version isn't after the current one, the tree is dirty, the branch isn't `main`, the tag exists or Unreleased is empty; if the checks fail after it has changed the version and changelog, it leaves them uncommitted and says so.
- 2026-10-28: `cargo xtask dist` works on Windows. What it took:
  - cargo-about 0.9.3's binary needs `--features cli`, and it refuses to write to a pipe from PowerShell, so dist has it write to a file with `-o`.
  - The MSVC linker writes the PDB's absolute path into each exe; `/PDBALTPATH:%_PDB%` leaves only its file name. The leak scan would have caught it.
  - Two clean builds differed until `/Brepro` (the linker's timestamp and PDB GUID). With it, binaries and zip are identical.
  - The `zip` crate records the OS it runs on as each entry's "made by" system, so a zip made on Windows read back with DOS permissions. Entries are now made on Unix.
  - cargo-about lists every crate in the workspace, its own included; `[private] ignore` drops ours, which are all `publish = false`. Generating for the whole workspace also lists xtask's own dependencies (`tar`, `toml_edit`): more notices than strictly needed, never fewer.
  - The archive is about 10 MB for Windows; the viewer is 17 MB of it unpacked.
- 2026-10-28: the workspace version isn't set to `0.1.0` ahead of the release. If `main` said `0.1.0` before the tag existed, every untagged build would claim to be that release. `cargo xtask release` sets the version in the release commit, as ADR 0037 has it, so between releases `main` carries the last release's version and `--version` adds `(dev build)`.
- 2026-10-28: unchecked versions can't be tested end to end with a relabelled copy of a real bundle: each structure template records its data version, and loading refuses one that differs from the bundle's. The warning's logic and wording are unit-tested in `spyglass-data` and `spyglass`, the JSON wiring in the CLI's `json` module, and an ignored CLI test checks that a checked version prints no warning. The extractor warns by id at the jar step, before downloading, since the data version is only known from the jar.
- 2026-10-28: the per-user folder is `ProjectDirs::data_local_dir()`, not `data_dir()`: on Windows that's `%LOCALAPPDATA%`, not the roaming profile, which shouldn't carry hundreds of megabytes of bundles. The two are the same elsewhere. The test drives the choice through a private function that takes what it found, since setting an environment variable in a test is `unsafe` in edition 2024 and the workspace forbids `unsafe`. `doctor` reports the folder too.
- 2026-10-28: drafted. Open decisions for the owner:
  - the target list, in particular whether Intel macOS and ARM Linux (`ubuntu-24.04-arm`, free for public repositories) are worth their jobs;
  - unsigned binaries for now;
  - `0.1.0` as the first version.
