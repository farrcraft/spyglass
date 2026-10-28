# 0033. The oracle accepts the Minecraft EULA only when asked

- Status: Accepted
- Date: 2026-10-28
- See also: [0015](0015-locate-checks-on-a-real-server.md), which this refines

## Context

[ADR 0015](0015-locate-checks-on-a-real-server.md) checks `/locate` answers on a real dedicated server (`cargo xtask oracle locate`, and `cargo xtask ci`, which runs it). Mojang's server won't start until `eula.txt` in its folder says `eula=true`: running it means accepting the [Minecraft EULA](https://aka.ms/MinecraftEULA). The oracle wrote that file into every throwaway server folder, so anyone running the command accepted the EULA without being told, and no doc said so. The in-process harness of [ADR 0004](0004-oracle-standalone-harness.md) starts no server and needs no `eula.txt`, which is why that ADR could say there was no EULA step. The [research note](../research/oracle-approaches.md) already said a dedicated server's EULA would have to be "accepted by whoever runs it".

The acceptance belongs to the person running the server, not to spyglass. But CI runs the command unattended, and so do contributors repeating `cargo xtask ci` locally, so the acceptance has to be expressible without a prompt.

## Decision

We will start the dedicated server only when whoever runs the command accepts the EULA explicitly: `--accept-eula` on `oracle locate` or `ci`, or `SPYGLASS_ACCEPT_EULA=1` (or `true`) in the environment. Without either, both commands stop before doing any work, with a message that links the EULA. The CI workflow sets the variable on its oracle jobs, with a comment saying what it accepts.

## Consequences

- No one accepts the EULA by running a command whose name doesn't say so. The message names the EULA and the two ways to accept it.
- `cargo xtask ci` checks before extracting, so a missing acceptance fails in a second, not after minutes of golden files.
- The rest of the oracle (`golden` and its parts, `test-pack`, `check-save`) and the extractor start no server and are unchanged.
- One more thing to know when running the locate oracle: [getting-started](../getting-started.md#6-the-oracle) and [oracle.md](../specs/oracle.md#locate-answers) say it.
- The variable is a standing acceptance for a shell or a CI job. That is the point, but it means CI's acceptance lives in the workflow file and is reviewed like any other change there.

## Alternatives considered

- **Keep writing `eula=true` and document it:** honest, but still accepts on the user's behalf by default.
- **An interactive prompt:** asks at the right moment, but breaks CI and any unattended run, so a flag or variable would be needed anyway.
- **Ask the user to write `eula.txt` themselves:** the server folders are throwaway and recreated per run, so there's nowhere stable for the user's file to live.
