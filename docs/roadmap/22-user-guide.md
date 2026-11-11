# 22. User guide

- Status: In progress
- Depends on: [11](completed/11-releases.md), [12](completed/12-first-run.md)
- Plans: [m22-user-guide](../plans/m22-user-guide.md)

## Goal

Someone who downloads a release, and never clones the repo, can learn everything the programs do from one website: installing, game data, the viewer, the `spyglass` command, their own worlds, searches and loot. The site describes the latest release, not `main`. Contributor docs stop mixing in user material. [ADR 0066](../adr/0066-a-user-guide-on-github-pages.md) records the decision.

## Scope

- **The book.** `docs/guide/` as an mdBook: `book.toml`, `SUMMARY.md` and pages. The likely chapters:
  - installing: the archives, unsigned-binary warnings on Windows and macOS, building from source as an option;
  - game data: Java, extracting a bundle from the viewer or the command line, where bundles go, more than one version;
  - the viewer: layers, markers and saved seeds, shared views, image export;
  - the `spyglass` command: locate and nearest, maps, seed search, search within a seed, loot, reading the answers;
  - your own worlds;
  - troubleshooting and FAQ: missing Java, a version not supported, what "certain" means for features.
- **Moving content.** getting-started.md §8–10 move into the guide and getting-started.md keeps contributor setup. The README's Install and Documentation sections link to the site. `GETTING-STARTED.txt` shrinks to an offline quick start that links to it.
- **Screenshots** from [docs/guide/src/images/](../guide/src/images/), moved there from `docs/images/`, and new ones taken with the viewer, from a seed and settings the page states.
- **Tooling.**
  - mdBook pinned in both setup scripts, and checked by `cargo xtask doctor`.
  - `cargo xtask guide [--serve]` builds the book, or serves it locally.
  - `cargo xtask check` builds the book.
  - `cargo xtask links` covers `docs/guide/`, and links leaving the guide are absolute.
- **Publishing.** A `pages.yml` workflow builds the guide from the release's tag and deploys it when a release is published, and can be run by hand. CI builds it on every pull request. [releasing.md](../releasing.md) gets the steps, including turning on Pages once.
- **Release notes.** The draft release's text links to the guide.

Not in scope:

- A guide for each release. One site, for the latest release.
- Library API docs (rustdoc) on the site. They can follow if the library gets users outside the repo.
- A custom domain, theme work beyond mdBook's defaults, and translations.
- Restating specs. The guide links to [cli-json.md](../specs/cli-json.md) and [search.md](../specs/search.md) instead of copying them.

## Exit criteria

- [x] The guide is published on GitHub Pages from the latest release's tag, and the README, the releases page and the archive link to it.
- [x] Every user-facing section of getting-started.md now lives in the guide, and getting-started.md covers only contributor setup.
- [ ] Every example command in the guide was run against the released build and gave the output shown.
- [ ] Someone new to spyglass follows the guide on a clean machine, from download to a seed search, with no other docs.
- [x] `cargo xtask check` builds the guide and passes, and CI builds it on pull requests.
