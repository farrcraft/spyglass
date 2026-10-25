---
name: adr
description: Create, supersede or update an Architecture Decision Record in docs/adr/. Use when the user wants to record a design decision, asks "should we write an ADR", proposes changing a past decision, or makes a choice with lasting consequences (a dependency, a data format, a crate boundary, an MSRV policy, a correctness strategy).
---

# Architecture Decision Records

ADRs live in `docs/adr/`, one file per decision, named `NNNN-kebab-case-title.md` with a four-digit, zero-padded, never-reused number. `docs/adr/README.md` is the index. `docs/adr/template.md` is the template.

If the skill was invoked with arguments, treat them as the decision's topic or title: $ARGUMENTS

## Creating an ADR

1. **Find the next number.** List `docs/adr/` and take the highest `NNNN-` prefix plus one. Ignore `README.md` and `template.md`.
2. **Gather the substance before writing.** An ADR needs:
   - the problem and the forces at play (constraints, requirements, what makes it hard),
   - the decision, stated in one or two sentences in active voice ("We will..."),
   - the consequences, good and bad,
   - the alternatives seriously considered and why each lost.

   Pull these from the conversation, the code and existing docs (`docs/architecture.md`, `docs/specs/`, `docs/research/`). If the decision itself or the main alternative is unclear, ask the user. Don't invent rationale.
3. **Write the file** from `docs/adr/template.md`. Set the date to today (ISO `YYYY-MM-DD`). Status is `Proposed` unless the user says the decision is already made, in which case use `Accepted`.
4. **Add a row to the index** in `docs/adr/README.md`, keeping it sorted by number.
5. **Cross-link.** If a spec or doc describes the behavior this decision governs, add a short "See ADR NNNN" link there. If the decision changes something stated in `CLAUDE.md` or `docs/architecture.md`, update that text too.
6. **Don't commit** unless the user asks.

## Superseding an ADR

Accepted ADRs are immutable history. Don't rewrite their Context or Decision. To change a decision:

1. Write a new ADR as above, with `Supersedes: [NNNN](NNNN-title.md)` in its header.
2. In the old ADR, change only the status line to `Superseded by [MMMM](MMMM-title.md)`.
3. Update both rows in the index.

Fixing typos, broken links or adding a "See also" to an old ADR is fine.

## Status changes

- `Proposed` → `Accepted` or `Rejected`: edit the status line and the index row. Rejected ADRs stay in the repo; the reasoning is still useful.
- `Accepted` → `Deprecated`: the decision no longer applies but nothing replaces it. Add one sentence under Status saying why.

## Writing style

- Keep it short; most ADRs fit on one screen. Link to specs and research for detail instead of repeating it.
- Write Context so a reader a year from now understands why this was hard without having been in the conversation.
- Name concrete things: crate names, versions, file paths, game versions.
- Consequences must include the costs. An ADR with only upsides hasn't finished thinking.

## When an ADR is warranted

Suggest one (don't just write it) when a change:
- adds or removes a significant dependency, or picks one library over another,
- changes crate boundaries or the dependency direction between crates,
- defines a file or data format (bundles, golden files, filter language),
- sets a policy (MSRV, supported game versions, what gets committed),
- picks a correctness or testing strategy.

Routine implementation choices don't need one.
