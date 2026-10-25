---
name: rust-review
description: Review Rust code in the spyglass workspace for Java bit-exactness, project rules, idiom and performance. Use when the user asks to review Rust code, a diff, a branch or a crate, or asks "is this port correct" / "does this match Java".
---

# Rust code review for spyglass

spyglass ports Minecraft: Java Edition world generation and must match it **bit for bit**. Most bugs that matter here compile cleanly, pass clippy and look reasonable. Review with that in mind.

## Scope

Work out what to review, in this order:

1. Paths, a crate name, a commit range or a branch passed as arguments: $ARGUMENTS
2. Otherwise, uncommitted changes plus commits on the current branch that aren't on `main` (`git diff main...HEAD` and `git diff HEAD`).
3. If there's nothing, ask what to review.

Read the changed code in full, plus enough of its callers and callees to judge it. Read the relevant spec in `docs/specs/` and any ADR in `docs/adr/` that governs the area.

## Mechanical checks

Run `scripts/check.ps1` (Windows) or `scripts/check.sh`. Report failures, but don't repeat individual clippy warnings in the review. Point to the command output instead.

## Checklist

### 1. Java parity (highest priority)

These are correctness bugs, not style issues.

**Integers**
- Java `int`/`long` arithmetic wraps. Rust's `+ - *` panic on overflow in debug and wrap in release. Any arithmetic that can overflow in Java (LCG steps, hashing, seed mixing, coordinate scrambling) must use `wrapping_*`.
- Shifts: Java masks the shift count (`& 31` for int, `& 63` for long). Rust panics on overshift in debug. Use `wrapping_shl`/`wrapping_shr` when the count isn't a constant in range.
- `>>` vs `>>>`: Java's `>>` on a signed type is arithmetic; `>>>` is logical. In Rust, `>>` on `i32`/`i64` is arithmetic; `>>>` needs `((x as u32) >> n) as i32` (or `u64`/`i64`). Check every port of `>>>`.
- Type widths must match the Java declaration: `int` → `i32`, `long` → `i64`, `short` → `i16`, `byte` → `i8`, `char` → `u16`. Using `i64` "to be safe" changes overflow behavior.
- `Math.floorMod(a, b)` / `Math.floorDiv(a, b)` are not `rem_euclid`/`div_euclid` when `b` is negative. They match only for positive divisors; check the sign.
- Java `/` and `%` truncate toward zero, the same as Rust's. Don't "fix" them to euclidean.
- Implicit widening: Java promotes `int * int` to `int` (wrapping) even when assigned to `long`. Rust code that casts to `i64` first gives a different answer. Cast where Java casts, not earlier.

**Floating point**
- `float` → `f32`, `double` → `f64`. Java promotes `float op double` to `double` implicitly; Rust needs an explicit cast, so check it happens at the same point.
- Operation order: `a * b + c * d` and `(a + b) + c` vs `a + (b + c)` must follow the Java source exactly. Flag any algebraic "simplification" of ported math.
- No `mul_add` unless the Java uses `Math.fma`. FMA rounds once instead of twice.
- Casting double to int: Java `(int) d` truncates toward zero, saturates and maps NaN to 0. Rust `as i32` does the same, so `as` is correct here; `round()`, `floor() as i32` are not substitutes unless Java does that. Minecraft's `Mth.floor` is `(int) d` with a correction for negatives; port it exactly.
- **Transcendental functions** (`sin`, `cos`, `exp`, `ln`, `pow`, `atan2`, ...): Java's `Math.*` may differ from Rust's platform libm in the last bit, and `StrictMath.*` is fdlibm. Flag every use in generation code and ask how parity is ensured (oracle coverage, an fdlibm port, or a lookup table as the game uses for `Mth.sin`). `sqrt`, `abs`, `floor` and `ceil` are exact in both and fine.

**Behavior**
- Iteration order: Java `HashMap` iteration order is not Rust's. If the Java code iterates a map or set and order affects RNG consumption or output, the port needs an ordered structure that reproduces Java's order (often `LinkedHashMap` → `IndexMap`, or a sorted list).
- RNG consumption: every call that advances an RNG in Java must advance it in the port, including calls whose results are discarded and calls inside branches. A skipped `nextInt` shifts every later value.
- String hashing and MD5 seeding must use the exact bytes Java uses (UTF-8 for MD5 of names, UTF-16 code units for `String.hashCode`).

### 2. Project rules (from CLAUDE.md)

- No Mojang code or data committed: no jars, extracted JSON, generated bundles, golden files or large verbatim decompiled source. Short references to class and method names in comments are fine and encouraged.
- Version-specific values belong in extracted data (`spyglass-data` bundles), not Rust constants. Algorithm constants that are fixed across all supported versions (LCG multiplier, Xoroshiro rotations) are fine in code.
- Dependencies flow downward per `docs/architecture.md`. Flag any new edge that points up or sideways.
- New dependencies go in `[workspace.dependencies]` and are referenced with `.workspace = true`. A significant new dependency probably warrants an ADR (see the `adr` skill).
- Code must not reference `docs/roadmap/` or `docs/plans/`, by path or by milestone or plan name ("see milestone 3", "per the m02 plan"), in source, comments, tests or manifests. These are temporary process docs (see `docs/development-process.md`). The durable explanation belongs in the code itself, a spec or an ADR. `scripts/check` catches paths but not names, so look for names by hand.
- Behavior covered by a spec should match the spec; if the code is right and the spec is wrong, the spec needs updating in the same change.

### 3. Tests

- Expected values must come from the game (the oracle, or values computed by running the actual Java), not from another third-party tool and not from running the Rust code itself and pasting the output.
- RNG and noise code needs tests that cover negative seeds and coordinates, overflow cases, and boundaries.
- If code has an optimized path and a plain reference path, a property test should check they agree.

### 4. Rust quality

- Library crates use `thiserror` error types; `anyhow` belongs only in binaries. No `unwrap`/`expect` in library code on input that can fail (malformed bundles, I/O); fine for real invariants, with the invariant stated in the `expect` message.
- `unsafe` needs a `// SAFETY:` comment and a reason safe code wasn't enough.
- Public items get doc comments. For ported functions, name the Java class and method they correspond to.
- Prefer newtypes for coordinate spaces (block vs quart vs chunk) where mixing them is a plausible bug.

### 5. Performance (hot paths only)

Noise sampling, density evaluation, biome lookup and the search loop run billions of times. In those paths, flag heap allocation per sample, `HashMap` lookups that could be array indexing, dynamic dispatch where the type set is closed, and redundant recomputation of per-seed state. Outside hot paths, don't comment on performance.

## Output

Group findings by severity, most severe first:

- **Parity:** produces different output from the game, or might. Always include the Java expression or behavior it diverges from and a concrete input where the results differ, if you can construct one.
- **Bug:** wrong in another way (panics, wrong error handling, broken invariant).
- **Rules:** breaks a project rule above.
- **Suggestion:** idiom, clarity, performance.

Each finding gets a clickable `path:line` link, one sentence on the problem and one on the fix. Say explicitly which checklist sections you checked and found nothing, so "no findings" is distinguishable from "didn't look". Don't edit code unless the user asks for fixes.
