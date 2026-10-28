# 0035. CI checks Windows too

- Status: Accepted
- Date: 2026-10-28
- See also: [0028](0028-supported-versions-and-ci.md), whose CI this adds a job to

## Context

[ADR 0028](0028-supported-versions-and-ci.md) set up CI on GitHub Actions: a `cargo xtask check` job and one oracle job per supported version, all on `ubuntu-24.04`. But Windows is the main development OS, and code paths there are what CI tested least:

- `scripts/setup.ps1`, kept in step with `setup.sh` by hand;
- Java discovery in the Microsoft Store and Modrinth launchers' folders;
- path handling (separators, `\\?\` prefixes, case-insensitive matching);
- the CLI's console output and Ctrl-C handling.

Local hooks run on Windows, but only for whoever commits, and only for the files they touch. [ADR 0007](0007-dev-container-and-linux-check.md) brought routine Linux runs for the same reason, after the first one found two bugs.

## Decision

We will add a `check-windows` job on `windows-2025`: it runs `scripts/setup.ps1`, then `cargo xtask check`, with the same tool cache as the Linux check job. The oracle jobs stay Linux-only, since the game's answers don't depend on the OS spyglass runs on.

## Consequences

- Both setup scripts run in CI on every change, so drift between them shows.
- Every hook, test and doctest runs on Windows as well as Linux, including Windows-only code.
- Windows runners are slower and, for private repositories, cost twice a Linux runner's minutes. For a public repository on the free plan they cost nothing, at the time of writing. The job's timeout is 90 minutes against Linux's 60.
- `setup.ps1` must stay runnable without a prompt: no `-Oracle`, which uses winget, and no interactive steps.
- Tests that are slow or flaky on Windows now block merges, and have to be fixed rather than ignored.

## Alternatives considered

- **Linux only:** cheapest, and the oracle is OS-independent. But it leaves the main development OS checked only by whoever happens to commit.
- **A Windows oracle job too:** the game's answers are the same on any OS. The only OS-dependent part, platform `Math` results, is already handled by reporting both outcomes ([ADR 0012](0012-java-trig-results-that-depend-on-the-platform.md)).
- **A scheduled Windows run instead of one per change:** cheaper, but finds breakage later and away from the change that caused it.
