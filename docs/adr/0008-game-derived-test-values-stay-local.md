# 0008. Game-derived test values stay local

- Status: Accepted
- Date: 2026-10-25

## Context

Bit-exact parity is checked against the real game ([specs/oracle.md](../specs/oracle.md)). Biome golden files are already generated locally into the gitignored `/oracle-out`, and CLAUDE.md says fixtures derived from the game are generated locally, not committed, "unless a spec explicitly says otherwise".

Porting the RNG and noise needs the game's raw values too: random sequences, positional seeds, noise samples at chosen points. The oracle will dump them as _probe_ files. That raises the question of whether a small set of them may be committed, so that plain `cargo test` checks noise parity without a JDK, a server jar or an oracle run.

Forces:

- Probe values are outputs of Mojang's code run on Mojang's data. A few dozen floats are a small thing, but the project's rule is simple and the oracle exists to make committing unnecessary.
- Without committed values, a contributor who never runs the oracle gets no parity signal from `cargo test`, only from the ignored tests.
- Much of the RNG can be checked against sources that aren't Mojang's: `java.util.Random` from any JDK, the Xoroshiro128++ reference implementation, MD5's RFC 1321 test suite, and published SplitMix/Stafford constants.

## Decision

We will keep **every value derived from the game in locally generated files**: golden files and probe files, both under `/oracle-out`, never committed. Tests that compare against them are `#[ignore]`d and run with `cargo test -- --ignored` once the oracle has generated them.

Committed tests use only values from sources that aren't Mojang's: the JDK, reference implementations, RFCs and published test vectors, or values a test derives from first principles (for example, a property that must hold). No spec makes an exception for game-derived values.

## Consequences

- The rule stays one line long and needs no case-by-case judgement about how much game output is too much.
- Plain `cargo test` doesn't check game parity for noise, density functions or biomes. Parity needs a JDK and an oracle run (`cargo xtask oracle golden`, and the probe command when it exists). CI will need the same, which ADR 0004 already assumes.
- Each layer should still have committed tests for what can be checked without the game: reference vectors, edge cases, invariants and agreement between fast and reference paths. That keeps regressions visible to contributors without the oracle.
- A failing ignored test can only be reproduced by someone who runs the oracle locally. Probe and golden files are deterministic, so everyone who runs it gets the same values.

## Alternatives considered

- **A small committed set allowed by a spec:** gives `cargo test` a parity signal without the oracle. Rejected: it makes an exception to the no-Mojang-data rule for convenience, and the line between "a few dozen samples" and "a fixture" would need policing.
- **Committed values computed by a third-party port** (cubiomes and similar): no Mojang output involved. Rejected because correctness is checked against the game, not other tools (CLAUDE.md), and 26.3's noise differs from what those tools implement.
