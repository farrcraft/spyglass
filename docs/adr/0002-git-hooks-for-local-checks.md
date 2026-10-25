# 0002. Git hooks for local checks

- Status: Accepted
- Date: 2026-10-24
- See also: [0003](0003-cargo-xtask-for-project-commands.md), which moves the `scripts/check` wrapper to `cargo xtask check`; [0005](0005-extra-rust-checks.md), which adds lint, dependency, MSRV and spelling checks

## Context

The most expensive mistake a contributor can make in spyglass is committing Mojang code or data: a server jar, extracted worldgen JSON, a generated bundle, golden files or decompiled source. `.gitignore` covers `*.jar`, `/bundles` and `/oracle-out`, but `git add -f`, or files extracted to some other path, gets past it. Once such a commit is pushed, removing it means rewriting public history.

Beyond that, the repo already mixes Rust, TOML, Markdown, POSIX sh and PowerShell, and will add JSON, YAML (CI) and Java (the oracle harness). A lot of routine hygiene applies across all of them: consistent line endings (contributors are on Windows, macOS and Linux), trailing whitespace, final newlines, merge-conflict markers, formatting, valid JSON/YAML/TOML, shellcheck. Each of those is a solved problem with a maintained hook. Writing and maintaining our own versions would be wasted effort.

Existing checks live in `scripts/check.ps1` / `scripts/check.sh`: `cargo fmt --check`, clippy, tests, and the rule that code must not reference `docs/roadmap/` or `docs/plans/` (see [development-process.md](../development-process.md)).

Constraints:

- Contributors work on Windows, macOS and Linux. The setup scripts so far only install Rust.
- Checks should be defined once, so that hooks and CI (not set up yet) can't drift apart.
- Client-side hooks are opt-in and bypassable, so they are a fast-feedback layer, not enforcement. CI has to run the same checks.

## Decision

We will use [pre-commit](https://pre-commit.com), configured in `.pre-commit-config.yaml` with every hook repo pinned to a release. That file is the single definition of the checks. Hooks, `scripts/check` and CI all run it.

**Installation.** The setup scripts install [uv](https://github.com/astral-sh/uv) if missing and run `uv tool install pre-commit`. uv fetches its own Python, so contributors don't install Python themselves. Then `pre-commit install` enables both hook stages via `default_install_hook_types`. pre-commit installs Node for the Node-based hooks itself.

**Line endings.** A `.gitattributes` with `* text=auto eol=lf` makes LF the repository standard. The `mixed-line-ending --fix=lf` hook enforces it.

**Initial hooks, at commit time (fast):**

| Area                 | Hooks                                                                                                                                                                                                         |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hygiene              | `pre-commit-hooks`: `trailing-whitespace`, `end-of-file-fixer`, `mixed-line-ending`, `check-merge-conflict`, `check-case-conflict`, `check-executables-have-shebangs`, `check-shebang-scripts-are-executable` |
| Syntax               | `pre-commit-hooks`: `check-json`, `check-yaml`, `check-toml`                                                                                                                                                  |
| Mojang data          | `check-added-large-files --maxkb=1024`; local `language: fail` hooks rejecting `*.jar`, `*.class`, paths containing `data/minecraft/`, and `*.java` outside the oracle harness directory                      |
| Process docs         | local `pygrep` hook rejecting `docs/(roadmap\|plans)` under `crates/`                                                                                                                                         |
| Rust                 | local `cargo fmt --all --check`                                                                                                                                                                               |
| TOML                 | `taplo` format check (`ComPWA/taplo-pre-commit`)                                                                                                                                                              |
| JSON, YAML, Markdown | Prettier (`rbubley/mirrors-prettier`, since pre-commit's own mirror is archived)                                                                                                                              |
| Markdown             | `markdownlint-cli2`                                                                                                                                                                                           |
| Shell                | `shellcheck` (`shellcheck-py`, which bundles the binary)                                                                                                                                                      |

**At push time (slower):** local hooks for `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, using `stages: [pre-push]`.

`scripts/check` becomes a thin wrapper that runs `pre-commit run --all-files` for both stages. Its existing grep moves into the config.

## Consequences

- A maintained catalogue covers the routine checks, and adding a check is usually a few lines of YAML.
- The config format is the de facto standard. [prek](https://github.com/j178/prek), a Rust reimplementation, reads the same file, so contributors who'd rather avoid Python can use it without any change to the repo.
- New prerequisite: uv and pre-commit, installed by the setup scripts. First run per machine downloads hook environments, including Node for Prettier and markdownlint, which takes a minute or two. Later runs are cached.
- Hook versions need bumping. `pre-commit autoupdate` does it, but someone has to run it and review the diff. Pinned revs mean a hook can't change behavior under us.
- Turning on formatters for TOML, JSON, YAML and Markdown means one initial commit that reformats the existing files, and the Prettier and markdownlint configs need tuning so they don't fight each other.
- Contributors who don't run the setup scripts get no hooks. CI has to catch what they miss.
- The 1 MiB limit and the blocked paths are heuristics. Legitimate exceptions need an explicit `exclude` in the config.
- The pre-push stage gets slower as the test suite grows. It can drop to clippy only, leaving tests to CI, without revisiting this ADR.
- Not covered initially: PowerShell linting (PSScriptAnalyzer needs PowerShell on every platform) and Java formatting for the oracle harness. Add them when they're needed.

## Alternatives considered

- **Plain git hooks in `scripts/hooks/` via `core.hooksPath`:** no extra tooling, but every check beyond the Mojang ones would be hand-written and maintained by us. That's a poor trade given how many standard checks we want.
- **[prek](https://github.com/j178/prek) (v0.5.7) as the primary tool:** no Python needed, and actively developed, but pre-1.0 and much less established. Because it reads pre-commit's config, choosing pre-commit doesn't rule it out.
- **[lefthook](https://github.com/evilmartians/lefthook) (v2.2):** a single Go binary with parallel hooks and glob filters. It's a hook _runner_, not a catalogue: each of the checks above would have to be wired up to a separately installed tool.
- **[cargo-husky](https://github.com/rhysd/cargo-husky):** last released in January 2020 (1.5.0), and it installs hooks as a side effect of building.
- **CI only, no local hooks:** simplest, but a Mojang file would already be pushed by the time CI flags it, which is exactly the case we most want to prevent.
