# m22. User guide

- Status: In progress
- Milestone: [22. User guide](../roadmap/22-user-guide.md)
- Started: 2026-11-09
- Completed:

## Goal

Every exit criterion of milestone 22:

- the guide published on GitHub Pages from the latest release's tag, and linked from the README, the release notes and the archive;
- getting-started.md's user sections moved into the guide, so it covers only contributor setup;
- every example command checked against the released build;
- the guide followed by someone new, on a clean machine, from download to a seed search;
- the guide built by `cargo xtask check` and by CI on pull requests.

[ADR 0066](../adr/0066-a-user-guide-on-github-pages.md) governs it.

## Approach

Tooling first, so that every page written after it is built and link-checked. Then the content, then publishing. The publishing workflow has nothing to deploy until a release comes after the content.

### Layout

```text
docs/guide/
  book.toml          title, the repo URL (mdBook's edit and "view on GitHub" links), build-dir
  src/
    SUMMARY.md       the table of contents
    images/          screenshots, moved from docs/images/
    *.md             one page per chapter
```

- The book builds into `target/guide/` (`build-dir` in `book.toml`), which is already gitignored and swept like other build outputs.
- `docs/images/` moves into `docs/guide/src/images/` with `git mv`. The README links to the images there, and the guide links to them relatively. One copy of each image, and it gets published.
- The site lives at `https://farrcraft.github.io/spyglass/`.

### Chapters

The milestone's list, as pages:

| Page                 | From                                                                              |
| -------------------- | --------------------------------------------------------------------------------- |
| `introduction.md`    | The README's opening and Goals, for a user: what it does, local only.             |
| `install.md`         | `GETTING-STARTED.txt` §1, the README's Install; building from source last.        |
| `game-data.md`       | getting-started.md §8, and the viewer's first-run extraction.                     |
| `viewer.md`          | getting-started.md §10, with markers, saved seeds, shared views and export.       |
| `command.md`         | getting-started.md §9: locate, nearest, features, loot, heights, blocks.          |
| `maps.md`            | §9's Maps.                                                                        |
| `seed-search.md`     | §9's Seed search, with worked queries; the language itself links to the spec.     |
| `search-in-seed.md`  | §9's Finding places in one seed.                                                  |
| `your-worlds.md`     | §9's Your own worlds.                                                             |
| `answers.md`         | §9's Reading the answers: certain and candidate, JSON output (links to the spec). |
| `troubleshooting.md` | Java not found, an unsupported version, unsigned-binary warnings, FAQ.            |

The commands are shown as an installed user runs them: `spyglass locate ...`, not `cargo run --release -p spyglass-cli -- ...`. getting-started.md keeps one line saying that from a clone, `cargo run --release -p spyglass-cli --` stands in for `spyglass`, and links to the guide.

### Tooling

- **mdBook pinned** in `scripts/setup.sh` and `scripts/setup.ps1`, beside cargo-nextest and cargo-shear (`cargo install --locked mdbook@<version>`), using the latest release when the plan starts. CI's docs and check jobs install the same version. A test like `one_pre_commit_version` in `xtask/src/dist.rs` keeps the four pins in step.
- **`cargo xtask guide`** builds the book into `target/guide/` and fails on mdBook's warnings, such as a page that `SUMMARY.md` names but that doesn't exist. `--serve` runs `mdbook serve` for writing, and `--open` opens the result in a browser. It lives in a new `xtask/src/guide.rs`.
- **A `guide` commit hook** in `.pre-commit-config.yaml` runs `cargo xtask guide` when files under `docs/guide/` change. CI's docs-only job runs the commit hooks, so it builds the guide too, and `cargo xtask check` runs every hook.
- **`cargo xtask doctor`** reports a missing or mismatched mdBook, as it does for cargo-nextest and cargo-shear.
- **`cargo xtask links`** already checks every tracked `*.md`, so it covers the guide. It gains one rule: a relative link in `docs/guide/` must resolve inside `docs/guide/`, because nothing else gets published. Links to specs, ADRs and code use `https://github.com/farrcraft/spyglass/blob/main/...`.

### Publishing

A new `.github/workflows/pages.yml`:

- **Triggers:**
  - `release: types: [published]`, skipping pre-releases (`github.event.release.prerelease`), so the site matches the latest release.
  - `workflow_dispatch` with a `ref` input (a tag) for a manual redeploy.
  - The owner publishes releases by hand ([releasing.md](../releasing.md)), so the event fires. A release published with the workflow's own `GITHUB_TOKEN` wouldn't trigger it.
- **Jobs:** `build` checks out the tag, installs the pinned mdBook, runs `cargo xtask guide` and uploads `target/guide/` with `actions/upload-pages-artifact`. `deploy` runs in the `github-pages` environment with `pages: write` and `id-token: write`, and calls `actions/deploy-pages`. Actions are pinned by SHA, as in the other workflows. Workflow-level permissions stay `contents: read`.
- **One-time setup:** Settings → Pages → Source: GitHub Actions, recorded in releasing.md.
- **The first deploy:** run the workflow by hand from the latest release's tag. That tag has no `docs/guide/`, so the first site goes up with the first release after this plan. Until then, the README's link waits too.

### Links to the guide

- **README:** Install links to the guide, and Documentation gets a "User guide" entry first.
- **Release notes:** a line linking to the guide, added where `cargo xtask release-notes` writes them, so every draft has it.
- **`GETTING-STARTED.txt`:** cut to unpacking, the first run on Windows and macOS, getting the game data, and a link to the guide for everything else. It stays plain text and readable offline.
- **docs/README.md:** a row for the guide.

## Tasks

### Tooling

- [x] Pin mdBook in both setup scripts and CI, with a test keeping the pins in step.
- [x] `docs/guide/book.toml` and a `SUMMARY.md` with an introduction page, building into `target/guide/`.
- [x] `cargo xtask guide [--serve] [--open]`, failing on mdBook warnings.
- [x] The `guide` commit hook, with CI's docs job installing mdBook.
- [x] `cargo xtask doctor` checks mdBook.
- [x] `cargo xtask links`: relative links in `docs/guide/` stay inside it, with a test.
- [x] `git mv docs/images docs/guide/src/images`, and fix the README's image paths.

### Content

- [x] `install.md` and `game-data.md`.
- [x] `viewer.md`, with the four existing screenshots.
- [x] New screenshots for the viewer page: markers, loot and the This seed search. The existing four have no recorded settings. Each screenshot's seed, version and settings are in its alt text or caption, so it can be retaken with the `run-viewer` skill.
- [x] `command.md`, `maps.md`, `search-in-seed.md` and `your-worlds.md`.
- [x] `seed-search.md`, with worked queries for common goals: huts near spawn, a village by a biome, loot.
- [x] `answers.md` and `troubleshooting.md`.
- [x] getting-started.md reduced to contributor setup, with §9–10 replaced by a link to the guide and §8 cut to the commands a clone needs. Fix every link into the removed sections (`cargo xtask links` finds them).

### Publishing and links

- [x] `.github/workflows/pages.yml`.
- [x] The README, docs/README.md, `GETTING-STARTED.txt` and the release notes link to the guide.
- [ ] Once the site is live: the README's Install section links to the site, not the guide's source.
- [x] releasing.md: turning on Pages once, the deploy on publish, a manual redeploy.
- [x] CLAUDE.md: `docs/guide/` in Layout and `cargo xtask guide` in Commands. development-process.md: the guide is a durable doc, so it must not link to individual milestones or plans.

### Verification

- [ ] Every command in the guide run against a `cargo xtask dist` build of the release candidate, with its output matching the page.
- [ ] A clean-machine walkthrough: download, first run, a bundle, a seed search, following only the guide. Findings fixed.
- [ ] After the next release is published, the site is live from its tag, and the README's link resolves.

## Verification

- `cargo xtask check` passes, including the `guide` and `links` hooks.
- A pull request touching only `docs/guide/` runs the docs job, which builds the guide.
- `cargo xtask links` rejects a test page with a relative link to `../../specs/search.md`.
- The manual walkthrough and the command check above. Nothing about worldgen changes, so `cargo xtask oracle-test` isn't needed.
- The Pages deploy's run log, and the live site, after the first release that includes the guide.

## Durable outputs

- [ ] [ADR 0066](../adr/0066-a-user-guide-on-github-pages.md): matches what was built. A new ADR supersedes it if the publishing model changes.
- [ ] [releasing.md](../releasing.md): the Pages steps.
- [ ] [development-process.md](../development-process.md) and CLAUDE.md: where the guide lives, and that it's durable.
- [ ] [getting-started.md](../getting-started.md): contributor setup only.
- [ ] The guide itself.

## Notes

- **Screenshots** of loot, markers and the This seed search were taken with the `run-viewer` skill, seed 12345 in 26.3, with each one's view and settings in its caption. The draft's list of presets was wrong (it named "rare biomes" and "a base site"); it now lists the seven the viewer has. The four older screenshots don't record their settings.
- **Links before the site exists.** The README, getting-started.md and docs/README.md link to the guide's source (`docs/guide/src/*.md`), which GitHub renders and `cargo xtask links` checks. The README's Install section switches to the site's URL once the first release with the guide is published.
- **The example commands** were run against a release build of `main` with 26.3 (2026-11-09). One query in the draft was invalid: a biome can't be measured from a structure, since structure anchors are only for structure and slime counts. `spyglass find --help` described the target as "a structure, a biome or a grid point", without features or loot; fixed.
- **mdBook 0.5.4** exits with failure on a chapter `SUMMARY.md` names but that's missing (with `create-missing = false`), and says nothing about a missing image or a broken link. `cargo xtask links` covers those, but only for files git tracks, so a new page is checked once it's staged.
