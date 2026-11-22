# casemap

The living map of an app's use cases: one Markdown file per use case in `docs/flows/`, written and maintained by a coding agent. The Rust CLI and library check, render and diff those flows in the terminal.

- Type: my own open source project (MIT), in `~/Code/open-source-projects/casemap`.
- Stack: Rust 2024 (MSRV 1.88); library deps unicode-width + thiserror; CLI behind the `cli` feature (clap, clap_complete, anyhow, notify, serde_json).

## Commands

```bash
cargo build
cargo test
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
cargo insta review
cargo run -- check --strict
cargo run -- docs/flows/render-a-diagram.md                # render once
cargo run -- --watch docs/flows/render-a-diagram.md  # redraw on every save
```

Release: automatic. main only takes PRs with green CI; each merge updates the release-plz PR (version + CHANGELOG from conventional commits); its CI needs a maintainer's "Approve workflows to run" click, and merging it publishes to crates.io, tags `vX.Y.Z` and dispatches cargo-dist (`release.yml`) for binaries, installers and the Homebrew formula. Commit messages must be conventional (`feat:`, `fix:`, `chore:` …). Run `dist generate` after editing `dist-workspace.toml`.

## Code map

`parser` builds a diagram with sections and icons. `src/flow_image/` lays it out and draws SVG or PNG. `image_terminal` handles in-terminal graphics and temporary PNGs. `src/main.rs` dispatches catalog setup, validation, listing, diffing, proposals and rendering. The binary-only `hook` module handles agent Stop checks, warning memory and project hook configuration. The binary-only `onboard` module handles the first-run menu, agent detection and launch. `docs/renderer.md` covers rendering, export and themes. The `src/browse/` TUI runs behind the default-on `tui` feature and handles the activity map, step selection and image viewport.

`anchors` (anchor syntax, matcher, file walk), `markdown` (mermaid blocks in .md), `map` (map file parsing, grouping, map renderers), `diff` (union diagram + marks), `mermaid` (Mermaid writer), `flows` (catalog, compare, report), `git` (repository paths, ignore checks, merge base and files at a commit via the git binary), `coverage` (code classification, repository coverage and uncovered changes), `check` (catalog validation), `skill` (embedded casemap skill, source `skills/casemap/SKILL.md`).

`action.yml` is the composite GitHub Action for PR flow comments and job summaries. `.github/workflows/flows.yml` uses the local Action; `move-action-tag.yml` moves its `v0` tag after cargo-dist announces a `v0.*` release.

Docs: `README.md` is the front page and stays under 200 lines. Feature reference lives in `docs/*.md` (index `docs/README.md`), development and releasing in `CONTRIBUTING.md`, and the example catalog in `examples/commerce/`.
