# spyglass docs

| Doc                                              | What it covers                                                          |
| ------------------------------------------------ | ----------------------------------------------------------------------- |
| [getting-started.md](getting-started.md)         | Installing Rust, building, testing, running.                            |
| [architecture.md](architecture.md)               | The code-vs-data split, crate layers and how they depend on each other. |
| [development-process.md](development-process.md) | How milestones become plans and code, and where each kind of doc lives. |
| [roadmap/](roadmap/README.md)                    | Milestones (one file each), stretch goals and what is out of scope.     |
| [plans/](plans/README.md)                        | Implementation plans for milestones.                                    |
| [specs/](specs/)                                 | Behavior the code must match: data sources, bundles, the oracle.        |
| [adr/](adr/)                                     | Architecture Decision Records: why significant choices were made.       |
| [research/](research/)                           | Prior art and open questions. Background reading, not requirements.     |

**Specs vs ADRs vs research:** a spec says what spyglass _must_ do and is updated alongside the code. An ADR records why a decision was made and is never rewritten, only superseded. Research is reference material that informs decisions but doesn't bind them. Roadmap milestones and plans are temporary process docs: they move to `completed/` when done, and code must never reference them.
