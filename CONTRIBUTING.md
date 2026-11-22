# Contributing

casemap is a Rust crate: a library plus the `casemap` CLI. Pull requests go to `main`, which merges them only with green CI.

## Development

Rust 1.88 is the minimum supported version (MSRV); the crate uses edition 2024.
Run `cargo test` for the full suite and `cargo test --no-default-features` for
library-only tests. Run `cargo fmt --all --check` and
`cargo clippy --all-targets --all-features -- -D warnings` before submitting changes.

The default features are `cli` and `tui`. `cli` enables clap, clap_complete, anyhow, notify and serde_json; `tui` adds ratatui only to the binary.
Run `cargo test --no-default-features --features cli --locked` to test the CLI without the browser.
With `default-features = false`, normal library dependencies are only
unicode-width and thiserror.

Review snapshot updates with `cargo insta review` (requires cargo-insta), or run
`INSTA_UPDATE=always cargo test` and inspect the diff before accepting changes.

To install from a local checkout, run `cargo install --path .`.

## Common commands

```bash
cargo build
cargo test
cargo test --no-default-features
cargo test --no-default-features --features cli --locked
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- check --strict                     # casemap's own flows in docs/flows
cargo run -- docs/flows/render-a-diagram.md          # render once
cargo run -- --watch docs/flows/render-a-diagram.md  # redraw on every save
```

## Tests

`cargo test` checks that flow cards, branches and labels fit the image layout
across the fixture corpus. It also tests terminal encoders, export, catalog
validation and CLI behavior against temporary repositories. Install `git` to run
the integration tests.

## Where things live

`parser` builds a diagram with sections and icons. `src/flow_image/` lays it out and draws SVG or PNG. `image_terminal` handles in-terminal graphics and temporary PNGs. `src/main.rs` dispatches catalog setup, validation, listing, diffing, proposals and rendering. The binary-only `hook` module handles agent Stop checks, warning memory and project hook configuration. The binary-only `onboard` module handles the first-run menu, agent detection and launch. [docs/renderer.md](docs/renderer.md) covers rendering, export and themes. The `src/browse/` TUI runs behind the default-on `tui` feature and handles the activity map, step selection and image viewport.

`anchors` (anchor syntax, matcher, file walk), `markdown` (mermaid blocks in .md), `map` (map file parsing, grouping, map renderers), `diff` (union diagram + marks), `mermaid` (Mermaid writer), `flows` (catalog, compare, report), `git` (repository paths, ignore checks, merge base and files at a commit via the git binary), `coverage` (code classification, repository coverage and uncovered changes), `check` (catalog validation), `skill` (embedded casemap skill, source `skills/casemap/SKILL.md`).

`action.yml` is the composite GitHub Action for PR flow comments and job summaries. `.github/workflows/flows.yml` uses the local Action; `move-action-tag.yml` moves its `v0` tag after cargo-dist announces a `v0.*` release.

## Commits and pull requests

- Commit messages are conventional (`feat:`, `fix:`, `docs:`, `chore:` …), because release-plz builds the version and the CHANGELOG from them.
- casemap maps itself: when behaviour changes, update the flows in `docs/flows/` in the same change, and keep `cargo run -- check --strict` passing.
- `README.md` is the project's front page and stays under 200 lines; reference for a new feature goes in a `docs/` page.

## Releasing

Releases are automatic from `main`, which only accepts pull requests with green CI:

1. Every merge to `main` updates a **release PR** opened by [release-plz](https://release-plz.dev). It bumps `version` from the conventional commits since the last release (`fix` bumps the patch, `feat` the patch while the version is 0.x, a breaking change the minor), and adds their entries to `CHANGELOG.md`. Edit the PR if the notes need it.
2. To release, open the release PR and click **Approve workflows to run**. GitHub holds CI on pull requests opened by GitHub Actions until a maintainer approves it. Once CI is green, merging the PR publishes the crate to crates.io (trusted publishing, no stored token) and tags `vX.Y.Z`.
3. The tag starts the [cargo-dist](https://github.com/axodotdev/cargo-dist) workflow (`.github/workflows/release.yml`). It builds the binaries, the shell and PowerShell installers and the Homebrew formula, and publishes the GitHub release with the changelog section as its notes.

The tap [Ross-cripto/homebrew-tap](https://github.com/Ross-cripto/homebrew-tap) copies the new formula with its daily Sync workflow; run it by hand (`gh workflow run sync.yml -R Ross-cripto/homebrew-tap`) to publish at once. After editing `dist-workspace.toml`, run `dist generate` to refresh the release workflow. Configuration: `release-plz.toml`, `.github/workflows/release-plz.yml`.
