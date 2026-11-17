# 0077. Open-ended public enums are non-exhaustive

- Status: Accepted
- Date: 2026-11-16

## Context

The `spyglass` crate's public enums are of two kinds. Some list things spyglass may learn more of: why it can't answer for a save, why a pack can't be applied, what a warning is about. Others describe a fixed set: the three dimensions, the answers a data pack can change.

The first kind grows. Milestone 31 took `SaveWarning` from seven variants to nine (`MovingBorder`, `PackError`). Each new variant breaks every `match` on the enum outside the crate that lists all the variants. The crates are `publish = false`, and [ADR 0037](0037-versions-and-releases.md) allows a minor release to break the API while the version starts with `0.`, so this is allowed. But it costs every caller a change each time, and almost no caller needs to treat each kind on its own. The CLI and the viewer use the enum's methods (`kind`, `subject`, `is_note`, `blocks_answers`, `Display`) and have a `_` arm where they match.

The crate already marks several such enums `#[non_exhaustive]` (`Error`, `InSave`, `PortalNote`, `ContainerSource`, `Carvers`, `Neighbours`), but no decision says when, so `SaveWarning` and `PackConcern` were left without it.

## Decision

We will mark a public enum `#[non_exhaustive]` when it lists things spyglass may learn more of: kinds of warning, concern, error, source, note or subject. An enum that describes a fixed set (the dimensions, a fixed set of answers or modes that only a redesign would change) stays exhaustive, so callers' matches stay checked.

Applied in milestone 31 to `SaveWarning`, `PackConcern`, `SaveSubject`, `Setting` and `PackUse`. `Answer` stays exhaustive.

## Consequences

- A new variant of an open-ended enum breaks no caller, and the release that adds one needs no breaking-change note for it.
- Callers outside the crate must have a `_` arm. A new variant then falls into it unnoticed: the compiler no longer points to the code that should handle it. Callers that need every kind should go through the enum's methods (`kind`, `Display`, `subject`), which the crate keeps complete.
- Making an enum non-exhaustive is itself a breaking change, once. For these five it lands in the same release that already breaks `SaveWarning`.
- Judging "open-ended" is a call made per enum; a wrong call is fixed by adding or removing the attribute in a minor release.

## Alternatives considered

- **Leave every enum exhaustive until 1.0:** callers keep compile-time checks, but each new warning or concern breaks them, for enums that grow with every limitation found.
- **Mark every public enum non-exhaustive:** no enum ever breaks callers, but fixed sets such as `Answer` and `Dimension` lose the checks that catch a missed case, for no gain.
