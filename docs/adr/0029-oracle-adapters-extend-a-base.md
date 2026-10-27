# 0029. Oracle adapters can extend a base adapter

- Status: Proposed
- Date: 2026-10-26
- See also: [0027](0027-oracle-harness-across-versions.md), which this amends (an adapter is no longer compiled alone)

## Context

[ADR 0027](0027-oracle-harness-across-versions.md) split the oracle harness into a core and per-version adapters behind `GameApi`. It said each adapter is compiled with the core alone, and that small differences inside an adapter's range are runtime checks rather than copies.

The first update showed the gap. 26.4-snapshot-5 fails to compile against the 26.3 adapter at exactly six calls:

- `createResolverForChunk` returns `NoiseBiomeResolver`;
- `getBaseHeight` is gone;
- `getBaseColumn` returns another `NoiseColumn`;
- `GenerationContext.biomeResolver()` is `noiseBiomeResolver()`;
- `Structure.generate` lost a parameter;
- `NoiseChunk`'s constructor takes a `ContextMap`.

Runtime checks can't bridge a signature that doesn't compile. A standalone 26.4 adapter would copy about 750 lines of the 26.3 adapter to change six calls. Every later snapshot would do the same, and support is permanent ([ADR 0028](0028-supported-versions-and-ci.md)), so the copies would accumulate and drift.

## Decision

An adapter may **extend a base adapter**.

- **Naming the base.** An `extends` file in the adapter's directory holds the base adapter's directory name.
- **Compiling.** The harness compiles the core with the base's Java files, then the adapter's. A file in the adapter replaces the base's file of the same name. Bases can chain.
- **Keeping differences small.** Version-sensitive game calls in an adapter that others extend are kept in a small file of their own (`Calls.java`), so a new version usually replaces that one file.
- **Choosing the adapter.** Selection is unchanged. The adapter whose `data-versions` ranges hold the jar's data version is chosen, and its chain of bases comes with it. Ranges must still not overlap.

## Consequences

- **26.4 costs one small file.** Its adapter is a `Calls.java` of the six changed calls, plus `extends` and `data-versions`.
- **A base can't change freely.** A change to a base reaches every adapter that extends it, which is what lets a fix land once, but each one has to compile. CI's per-version matrix compiles every adapter, which catches a break.
- **Where code lives is a trade-off.** Code that moves into `Calls.java` to make overriding cheap is spread over two files. Only calls that have differed, or plainly will, go there.
- **The core is untouched.** It still sees only `GameApi`.

## Alternatives considered

- **A copy of the adapter per version** ([ADR 0027](0027-oracle-harness-across-versions.md) as written): copies 750 lines per snapshot to change six calls. It is the drift ADR 0027 meant to avoid.
- **Reflection for the six calls:** one adapter for 26.3 and 26.4. But the harness's correctness would rest on method names looked up at run time, and failures would surface deep in a run. Reflection stays for private members, as before.
- **Java inheritance (`Adapter26_4 extends Adapter26_3`):** expresses the override in the language. But the base must still compile against the newer jar, which it doesn't, so the override has to be at the file level.
