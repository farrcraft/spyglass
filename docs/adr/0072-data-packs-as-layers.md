# 0072. Data packs as layers over the bundle

- Status: Accepted
- Date: 2026-11-11

## Context

A world's data packs change its generation by replacing or adding to vanilla's worldgen files, and until now spyglass only warned about them ([world-saves.md](../specs/world-saves.md#warnings)). spyglass keeps worldgen parameters in data, and a bundle is laid out as a data pack already, so a pack's files could be read in place of the bundle's. But the game doesn't simply overwrite files ([data-packs.md](../specs/data-packs.md#the-games-rules)):

- the highest pack's file wins;
- a pack's `filter` hides lower packs' files;
- overlays are picked by the version's data pack format;
- tags merge across packs instead of replacing, and `replace` clears what's below.

Every loader in spyglass reads through four `Bundle` methods, and caches tell worlds apart by their bundle.

Packs are untrusted input, as saves are. Filter patterns are Java regexes. And checking spyglass against the game with packs needs packs that override vanilla files, which must start from vanilla's, and vanilla data can't be committed ([ADR 0008](0008-game-derived-test-values-stay-local.md)).

## Decision

We will apply packs as layers inside `Bundle`. `Bundle::with_packs(packs)` returns a bundle whose `resource`, `registry_ids` and `structure_template` look down through the packs as the game's resource manager does, merge tags as `TagLoader` does, and pick each pack's overlays by the data pack format the extractor now records. Nothing below `Bundle` changes.

- **Filters:** `regex-lite` reads filter patterns, matched anywhere in the id as `Pattern.asPredicate` does. A pattern it can't read is an error, not a guess.
- **Formats:** a pack whose formats don't cover the version is applied anyway, as the game applies it, with a note.
- **Errors up front:** a world with packs loads all its data on opening, and fails only on errors the bundle without the packs doesn't have.
- **The oracle:** it checks pack worlds with packs of its own. The test pack, now loaded over the plain bundle, and a fixture pack that `cargo xtask oracle fixture-pack` generates from the user's own bundle.

## Consequences

- Saves' `file/` packs and the bundle's built-in feature packs apply to `--world` answers and the viewer, and `--pack` applies packs for a seed. Warnings narrow to what can't be applied.
- **One read path:** the probe, comparisons and front ends all read packs through the same reader, so the oracle checks the overlay itself. The merged test-pack bundle in `target/oracle/bundles/` is gone.
- **Costs:**
  - Every bundle must be extracted again to record its data pack format; without it, packs are refused.
  - A pack is held in memory while it's applied: up to 512 MiB under the limits, a few MB for real packs.
  - Opening a world with packs takes as long as a first query of every kind.
  - Tag entries naming ids that don't exist are kept, where the game drops the whole tag, since a bundle has no registries to check against.
  - `regex-lite` is a new dependency, and its syntax isn't all of Java's: lookaround and backreferences are refused.

## Alternatives considered

- **Merged bundles on disk**, as the test pack had: a copy of the bundle per set of packs, re-hashed. It can't express filters or tag merging without reimplementing them at merge time anyway, it costs a bundle's worth of disk per world, and stale merges go unnoticed.
- **`regex`** for filters: complete Unicode support and faster matching, at many times the build size and compile time, for patterns that are a few characters matched against a few thousand paths once.
- **Refusing packs of another format**: simpler to explain, but wrong. The game loads them, and the world was generated with them.
- **Checking each pack file with its own loader** instead of loading the world: the loaders are top-down from the dimension, with no per-file entry points for most registries, and a file no dimension uses can't change answers.
- **A committed fixture pack** of hand-written overrides: overriding the noise settings or a vanilla density function means shipping vanilla's JSON, which ADR 0008 rules out.
