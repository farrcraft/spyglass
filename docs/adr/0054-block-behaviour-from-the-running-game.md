# 0054. Block behaviour from the running game, by a committed dumper class

- Status: Proposed
- Date: 2026-10-30

## Context

Placing features reads per-state block behaviour that no data report has: `isSolid` (the legacy solid flag, computed from the block's collision shape), `liquid`, `canBeReplaced`, `isSolidRender`, the fluid state, and in 26.2 `blocksMotion` and whether a block is leaves (26.2's heightmaps). A lake declines on a non-solid edge, a dungeon counts solid walls, and the heightmaps that place a desert well depend on what blocks motion. These are the game's code, not its data: `BlockBehaviour` computes them from shapes and per-block overrides when the game boots. `reports/blocks.json` lists states and properties only.

The extractor runs only Mojang's data generator (`net.minecraft.data.Main --reports`) on the user's Java runtime. A JRE is enough, with no JDK ([data-sources.md](../specs/data-sources.md#java-runtime)), and end users extract bundles through the viewer ([ADR 0042](0042-the-viewer-extracts-bundles-itself.md)). So the flags must come from running the game, on a JRE, for every supported version. The hooks forbid committed `.class` files and Java outside `oracle/`, to keep Mojang code out of the repo.

## Decision

We will add a small Java program of our own, `extractor-java/BlockReport.java`. It calls the game only through reflection, so one compiled class works for every version. After the data generator has unpacked the server jar, the extractor runs it on the unpacked game and libraries. It bootstraps the game, walks `Block.BLOCK_STATE_REGISTRY`, and writes `reports/block_behaviour.json` into the bundle: each state's flags, keyed the way `reports/blocks.json` names states. It calls a method only if the version has it (`blocksMotion` exists only in 26.2).

The source and its compiled `BlockReport.class` are both committed. `cargo xtask java-report` rebuilds the class with `javac --release 21` and a fixed set of flags, so the output is reproducible. A check fails when the committed class isn't what the source compiles to, and the extractor embeds the class with `include_bytes!`. The hooks allow `.java` and `.class` files under `extractor-java/` only.

## Consequences

- Users still need only a Java runtime. Building spyglass from source needs no JDK; only changing the dumper does.
- The flags are the game's own in every version. A version that changes a block's shape or overrides needs no change in spyglass.
- A class file built from our source is committed, which the repo otherwise forbids. The source sits beside it, and the check means the binary can't drift from the source. Its bytes depend on the JDK's `javac`, so the check runs only where a JDK of the pinned major is installed (the oracle's, [ADR 0004](0004-oracle-standalone-harness.md)), and is skipped elsewhere with a message.
- The extractor boots the game a second time after the data generator, which adds a few seconds to an extraction.
- Reflection gives no compile-time check against the game. A renamed method fails at extraction, with the method named, rather than at build time.
- Bundles extracted before the report have no `reports/block_behaviour.json`. Placing asks for them to be extracted again, as other additions have ([bundle-format.md](../specs/bundle-format.md#layout)).

## Alternatives considered

- **Compile the dumper when spyglass is built** (`build.rs` running `javac`): nothing binary is committed, but everyone building spyglass would need a JDK as well as Rust.
- **A Rust table of flags per version,** checked against the oracle: version data in code, against "algorithms in code, parameters in data". It would have to be redone for every version and every block a feature can meet.
- **Recompute the flags in Rust from collision shapes:** the shapes are code too (per-block `getShape` overrides), so it's the same problem one level down.
- **Take the flags from the oracle's harness,** which already boots the game: the oracle needs a JDK and isn't part of extraction, so users' bundles wouldn't have them.
