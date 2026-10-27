# 0030. A test pack for forms the vanilla data doesn't use

- Status: Accepted
- Date: 2026-10-27
- See also: [0008](0008-game-derived-test-values-stay-local.md), [0026](0026-how-game-versions-coexist.md), [0027](0027-oracle-harness-across-versions.md)

## Context

26.4-snapshot-6 added three forms that its own vanilla data never uses: the `domain_warp` density type, gradient coordinates written as vertical anchors, and `corner_constraint` on jigsaw pool elements. [ADR 0026](0026-how-game-versions-coexist.md) says a data pack written for a supported version loads, because loaders accept every supported form. spyglass doesn't load data packs yet, but it does implement unused forms (`pow`, `cosine` upscaling) so that it can.

The oracle only checks what the game computes on its own data. For these forms, no golden file would ever exercise them. Hand-written tests can pin arithmetic, but they can't confirm game behavior that only the bytecode shows. Examples are how the game's `SLICE_UNIFORM_AXES` rewrite changes the volume a `domain_warp` sees, and the fact that a child rejected by its corner constraint has already taken its free space. Getting either wrong passes a hand-written test written from the same misreading.

## Decision

We will implement forms the vanilla data doesn't use, and check them against the game through a committed test data pack.

- The pack is `oracle/testpack/`, namespace `spyglass_test`. It is spyglass's own JSON and names only vanilla noises, functions and templates by id.
- The harness takes `--pack <dir>` before any command. It loads the pack over the vanilla one and records the version as `<version>+testpack`.
- `cargo xtask oracle test-pack` builds `target/oracle/bundles/<version>+testpack/` (the bundle plus the pack's files) and runs the probe and structure commands with the pack.
- `oracle-test` treats `<version>+testpack` as one more version, so the existing comparisons check the pack unchanged.

Decided with the user on 2026-10-27, over deferring the forms or testing them by hand only.

## Consequences

- The same machinery that checks vanilla data checks the new forms, bit for bit, at points, in columns and over chunks, and in structure layouts. The probe adds whole-chunk volumes for the pack's functions, since a warp's volume path depends on the volume's shape.
- [ADR 0008](0008-game-derived-test-values-stay-local.md) holds: the pack is written by hand, and every game-derived output stays local.
- Each version that adds an unused form adds to the pack, which only grows ([ADR 0028](0028-supported-versions-and-ci.md)'s permanence applies to it too). A pack form only parses from the version that introduced it, so the command runs from 26.4-snapshot-6 on, and the 26.2 adapter rejects packs.
- The merged bundle lives in `target/`, not `bundles/`, so the CLI and viewer never offer it. The CLI's comparisons, which look versions up in `bundles/`, skip the variant.
- It costs a minute and a half per version to generate, and the variant reruns the bundle-only comparisons.
- Implementing unused forms is real work per snapshot. 26.4-snapshot-6 needed `domainAxes`, anchor heights threaded into every binder, and a density sampler in structure assembly.

## Alternatives considered

- **Defer until vanilla data or data-pack loading uses them:** `version-diff` would flag the shapes when vanilla starts using them. But support for a version would be partial in a way nothing records. The work would also land later, under pressure, when the details are no longer fresh.
- **Hand-written tests only:** cheap, but they can't confirm game semantics that are invisible in the data, which is where these forms are hardest.
- **A separate harness command per form, with its own output format:** duplicates the probe and structure machinery, along with the Rust readers and comparisons.
