# 0042. The viewer extracts bundles itself

- Status: Accepted
- Date: 2026-10-29
- Amends: [0016](0016-extractor-crate-and-read-only-data.md), which allowed only `xtask` to depend on `spyglass-extract`
- See also: [0043](0043-bundles-beside-the-executables.md)

## Context

The viewer can't show anything until there's a bundle, and a bundle comes from the extractor. A downloaded viewer ([ADR 0037](0037-versions-and-releases.md)) opened by double-clicking gets a screen telling its user to run a command in a terminal, from a clone, with `cargo`. A release has neither, and many of the people who want a map viewer would rather not open a terminal at all.

`spyglass-extract` is already a library: `extract::extract` takes options and reports each step through a callback, `manifest::fetch_manifest` lists every version, and `JavaNotFound` says what was searched. [ADR 0016](0016-extractor-crate-and-read-only-data.md) moved it out of `spyglass-data` to keep network, TLS and archive code out of the library crates and the `spyglass` API. It also said nothing but `xtask` may depend on it, which rules out the viewer using it.

## Decision

We will let `spyglass-viewer` depend on `spyglass-extract` and extract bundles in-process. The viewer offers a game version picker, filled from Mojang's version list and marking the versions this build was checked against, and an Extract button that runs `extract::extract` on a background thread and shows its steps.

The library crates and the `spyglass` API still must not depend on `spyglass-extract`. Of the front ends, only the viewer and `xtask` do. The CLI keeps pointing at the `spyglass-extract` command, which ships beside it.

On Windows, the data generator's `java` runs with no console window of its own, since the viewer has none for it to share.

## Consequences

- A downloaded viewer gets its first bundle with no terminal. Errors arrive as typed values, so a missing Java runtime gets the same advice as on the command line, from one place in `spyglass-extract`.
- The viewer binary compiles and links `ureq`, `rustls`, `ring` and `zip`. It's bigger, and its builds are slower. The library crates are unaffected: `cargo tree -p spyglass -e normal` still shows no network code.
- The viewer now makes network requests, to Mojang's manifest and download servers, but only when its user opens the extraction picker or extracts. Nothing is sent but the requests themselves.
- An extraction can't be cancelled. Closing the viewer during one leaves the data generator's `java` running until it finishes, and its scratch directory in the temp folder.
- `docs/architecture.md`'s layering names the viewer as a user of `spyglass-extract`.

## Alternatives considered

- **Run the `spyglass-extract` binary as a child process:** no new dependency for the viewer, but progress and errors would be scraped from text, the version list would need a new subcommand with a machine-readable output, and it breaks whenever the extractor isn't beside the viewer, which is the case for `cargo run -p spyglass-viewer`.
- **Move extraction behind the `spyglass` API:** every embedder of the library would compile the TLS stack, which is what ADR 0016 removed.
- **Better instructions only:** name the shipped `spyglass-extract` command instead of `cargo`. That's still a terminal, and it's the step most first-time users get stuck on.
