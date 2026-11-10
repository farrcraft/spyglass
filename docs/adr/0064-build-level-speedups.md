# 0064. Build-level speedups: FxHash for hot maps, mimalloc, PGO for local builds only

- Status: Accepted
- Date: 2026-11-08
- See also: [0038](0038-how-release-archives-are-built.md), [0063](0063-threads-within-library-queries.md)

## Context

Profiles of chunk work, dungeon tiles and `nearest_feature` showed three costs that come from the build, not from the algorithms:

- **SipHash.** std's `HashMap` hashes with SipHash, which resists collision attacks on untrusted keys. The keys here are coordinates and binder slots. On 26.3, `corners_of`'s map in `spyglass-density` (a 52-byte key per point's corner cell) spent 5.9 to 7.1% of chunk and feature work hashing; on 26.2, `NoiseChunk`'s `surface_levels` and `columns` spent 1.9%. No other map of the workspace's hundred-odd passed 0.3%.
- **The system allocator under threads.** Since [ADR 0063](0063-threads-within-library-queries.md), feature queries carve and place chunks on every core, and each chunk allocates its volumes and caches.
- **Code layout and inlining** that the compiler chooses without knowing which paths are hot.

Each fix must leave answers unchanged; none touches arithmetic. Each must also fit how releases are built ([ADR 0038](0038-how-release-archives-are-built.md)): `--locked`, the pinned toolchain, four targets, and archives that a second build of the same commit reproduces byte for byte.

The machine they were measured on (16 threads) had a VPN client and a browser each keeping a core busy, so three bench rounds spread by about ±25%. Short lines ran eight alternating rounds of 40 runs each, and minimums and medians were compared. The numbers are in the milestone's plan ([plans](../plans/README.md)).

## Decision

We will:

1. **Hash those three maps with FxHash** (`rustc-hash` 2, `FxHashMap`), and leave every other map on std's hasher until a profile shows it. FxHash took about a tenth off 26.3's chunk work. None of the three maps is iterated, so the hasher can't change an answer.
2. **Use mimalloc as the global allocator** (`mimalloc` 0.1.52) in `spyglass-cli`, `spyglass-viewer` and the bench, which times what those binaries do. The library crates don't set one, so programs built on them choose their own. mimalloc took about 9% off 26.3's threaded dungeon tiles and didn't change single-threaded chunk work or 26.2.
3. **Offer PGO for this machine only:** `cargo xtask bench --pgo` and `cargo xtask dist --pgo` build the bench instrumented, train on it, merge the counts with `llvm-profdata` (rustup's `llvm-tools` component) and build with `-Cprofile-use`, under `target/pgo/`. PGO took 10 to 15% off 26.3's chunk work and more off its features, but 26.2's biome lines lost 5%. Release archives don't use it: a training run's counts depend on thread scheduling, so two builds of one commit would differ.

## Consequences

- 26.3's chunk work and threaded features are faster in every build, and faster again for anyone who builds with `--pgo`.
- `rustc-hash` was already in the tree, under licenses `deny.toml` allows. `mimalloc` (MIT) builds C with `cc`. The viewer already did, for `ring`; the CLI didn't. Every target's builder needs a C compiler, which the Rust targets we ship need for linking anyway. macOS builds are checked only by the release workflow.
- Released binaries and `--pgo` builds differ in speed. A slowdown reported by a user should be timed on a plain `dist` build before it's compared with ours.
- `--pgo` trains on the whole bench, instrumented: about 20 minutes for `dist --pgo` on a 16-thread machine. Its profile fits the bench's mix of work, so a gain on the bench overstates the gain elsewhere, and versions the bench didn't train on (26.2's biome lines) can lose.
- A map that becomes hot later needs its own profile and its own change; there's no workspace-wide hasher to switch.
- PGO for releases stays open. A training run that gives the same counts every time (one thread, a fixed set of lines, bundles extracted in CI) would remove the conflict with ADR 0038, at about ten minutes per target.

## Alternatives considered

- **FxHash for every map**, or `foldhash`: most maps are built once at load and read rarely. Changing them would be churn the profile doesn't support. FxHash beats foldhash on small integer keys, and `rustc-hash` 2 was already a dependency.
- **The allocator in the library crates:** a library can't set a global allocator without imposing it on every program that links it.
- **No allocator change:** 9% on the threaded work that ADR 0063 made common was judged worth a C dependency in the CLI.
- **PGO in releases now**, from a profile committed to the repo: the profile goes stale with every change to the code it describes, and it's a binary blob made from a run over the game's data.
- **`cargo-pgo`:** another tool to install for what three flags and one `llvm-profdata` call already do.
