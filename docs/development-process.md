# Development process

Work flows from roadmap milestones to implementation plans to code. Milestones and plans are **process documents**: they track work in progress and move when it's done. Durable knowledge lives elsewhere.

## Documents

| Kind                          | Where                     | Lifetime                                                                  |
| ----------------------------- | ------------------------- | ------------------------------------------------------------------------- |
| Milestone                     | `docs/roadmap/NN-slug.md` | Until its exit criteria are met, then moved to `docs/roadmap/completed/`. |
| Implementation plan           | `docs/plans/mNN-slug.md`  | Until implemented, then moved to `docs/plans/completed/`.                 |
| Spec                          | `docs/specs/`             | Durable. Updated alongside the code.                                      |
| ADR                           | `docs/adr/`               | Durable. Never rewritten, only superseded.                                |
| Architecture, getting started | `docs/`                   | Durable.                                                                  |

## Lifecycle

1. **Milestone.** Each milestone in [roadmap/](roadmap/) states its goal, scope and exit criteria. Its status is `Planned` until a plan starts, then `In progress`.
2. **Plan.** Before implementing, write one or more plans in [plans/](plans/) from [plans/template.md](plans/template.md). A plan is named after its milestone (`m01-bundle-format.md`) and links back to it; the milestone lists its plans. Large milestones get several plans, each small enough to finish and verify on its own.
3. **Implement.** Work through the plan, keeping its task list current. Decisions made along the way that will outlast the plan go into an ADR. Behavior the code must keep matching goes into a spec.
4. **Complete the plan.** When every task is done and verified, check that nothing durable lives only in the plan, then `git mv` it to `plans/completed/`, set its status to `Completed` and fix links to it.
5. **Complete the milestone.** When all its plans are complete and its exit criteria are met, `git mv` it to `roadmap/completed/`, set its status to `Completed`, update the table in [roadmap/README.md](roadmap/README.md) and fix links to it.

When moving a file, search the docs for its filename and update every relative link.

## Process docs are not referenced by code

Milestones and plans move and eventually stop mattering, so nothing durable may depend on them:

- **Code** (source, doc comments, tests, `Cargo.toml`, scripts) must not reference `docs/roadmap/` or `docs/plans/`, by path or by milestone or plan name. If code needs to explain itself, put the explanation in the code, a spec or an ADR, and reference that. A pre-commit hook rejects any file under `crates/` that mentions either directory.
- **Durable docs** (specs, ADRs, architecture, CLAUDE.md) may link to the [roadmap index](roadmap/README.md) or [plans index](plans/README.md), but not to individual milestones or plans.

Milestones and plans may link to anything, including each other.
