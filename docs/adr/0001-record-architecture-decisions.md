# 0001. Record architecture decisions

- Status: Accepted
- Date: 2026-10-24

## Context

spyglass will make decisions with long-lived consequences: bundle formats, crate boundaries, dependencies, how correctness is verified, which game versions are supported. The specs say _what_ the code must do, but not _why_ one approach was chosen over another. Without that record, decisions get relitigated or silently reversed.

## Decision

We will record significant decisions as Architecture Decision Records in `docs/adr/`, using the format in [template.md](template.md). Accepted ADRs are not rewritten. A changed decision gets a new ADR that supersedes the old one.

## Consequences

- Rationale survives beyond the conversation that produced it.
- Writing an ADR adds a small cost to significant changes. Routine implementation choices don't need one.
- Specs describe current behavior; ADRs explain how we got there. The two link to each other where relevant.

## Alternatives considered

- **Rationale in commit messages:** hard to find later and can't be superseded cleanly.
- **Rationale inside specs:** mixes what with why, and specs get edited as behavior changes, losing history.
