# 08. Nether, End and multiple versions

- Status: Completed, with two exit criteria carried as follow-ups ([below](#follow-ups))
- Depends on: [07](07-terrain-and-validity.md)
- Plans: [m08-dimensions](../../plans/completed/m08-dimensions.md), [m08-versions](../../plans/completed/m08-versions.md)

## Goal

Cover all three dimensions and more than one game version.

## Scope

- Nether and End biomes and structures, available through locate queries, search constraints, world saves and the viewer, with a dimension switch.
- Cross-dimension constraints, e.g. "a nether fortress within 100 Nether blocks of the Overworld spawn's portal position".
- Support for several game versions side by side, each with its own bundle.
- A documented process for adding a new version: extract, diff, fix what changed.

## Exit criteria

- [x] Nether and End outputs match the oracle.
- [ ] At least two versions pass the full oracle suite in CI. Four pass `cargo xtask ci` locally, and the workflow is ready; waiting for the repository to be published on GitHub.
- [ ] A spec or guide documents how to add a version, and has been followed at least once. [adding-a-version.md](../../adding-a-version.md) exists, written from 26.2's and the first two snapshots' additions; it's met when the next snapshot is added from it.

## Follow-ups

Moved to completed on 2026-10-27 with the two unchecked criteria above still open. They need no plan, and are listed in the [roadmap](../README.md#follow-ups) until done:

- the first green CI run on GitHub, once the repository is published there (criterion 2);
- adding each new 26.4 snapshot, pre-release, release candidate and the release by the guide, the first of which meets criterion 3.
