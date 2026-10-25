# 0011. Parse JSON numbers from their text

- Status: Proposed
- Date: 2026-10-25
- Amends: [0010](0010-typed-loading-of-worldgen-json.md), decision point 6

## Context

[ADR 0010](0010-typed-loading-of-worldgen-json.md) says the game reads JSON numbers with `Double.parseDouble` and then narrows to `float` where a codec asks for one, and has spyglass do the same through `serde_json`'s correctly rounded `f64` parsing (`float_roundtrip`). That isn't what the game does.

The game parses JSON with Gson, which keeps each number's text as a `LazilyParsedNumber`. Codecs then call `floatValue()`, `doubleValue()` or `intValue()` on it (`JsonOps.getNumberValue`):

- `floatValue()` is `Float.parseFloat(text)`: decimal text straight to the nearest `float`.
- `doubleValue()` is `Double.parseDouble(text)`.
- `intValue()` tries `Integer.parseInt(text)`, then `(int) Long.parseLong(text)`, then `new BigDecimal(text).intValue()`.

Parsing to `f64` first and then narrowing to `f32` rounds twice. That can differ from rounding the decimal straight to `f32` when the `f64` lands exactly halfway between two `f32`s. A check of all 288 distinct decimal literals in 26.3's worldgen JSON found none that differ, so 26.3 is unaffected. But `float` fields are common (density-function constants, gradient values, spline points), and a datapack or a later version can contain such a literal.

## Decision

We will keep every JSON number's original text and parse it the way the codec's `Number` method does:

- The workspace enables `serde_json`'s `arbitrary_precision` feature, so a `serde_json::Number` keeps its text.
- Loaders read numbers through shared helpers: `f32` fields with Rust's correctly rounded `str::parse::<f32>` on the text, `f64` fields with `str::parse::<f64>`, and `int` fields as `intValue()` does (`i32`, else `i64` narrowed with wrapping, else the decimal truncated toward zero).
- `float_roundtrip` stays enabled for `f64` fields deserialized through serde derive.

This replaces ADR 0010's point 6. The rest of ADR 0010 stands.

## Consequences

- Every number reaches spyglass exactly as it reaches the game, whatever its type, with no double rounding.
- `arbitrary_precision` makes every `serde_json::Value` number hold a string. That costs a little memory and parse time at load, and it's a global feature: code that relies on `Number`'s internal representation (untagged enums over numbers, for one) behaves differently. spyglass has none today.
- The number helpers become the one place where JSON numbers are read. Loaders mustn't call `as_f64()` on a value that the codec reads as `float`.

## Alternatives considered

- **Keep `f64` parsing and narrow (ADR 0010 as written):** simpler, and exact for every literal in 26.3, but wrong in general. The difference would show up as an unexplained one-ulp mismatch in some future datapack.
- **Use `serde_json::value::RawValue` per field:** keeps text without a global feature, but every loader would have to work with raw fragments rather than `Value`, which defeats the hand-written walk ADR 0010 chose.
