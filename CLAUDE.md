# casemap

The living map of an app's use cases: one Markdown file per use case in `docs/flows/`, written and maintained by a coding agent. The Rust CLI and library check, render and diff those flows in the terminal.

- Type: my own open source project (MIT), in `~/Code/open-source-projects/casemap`.
- Stack: Rust 2024 (MSRV 1.88); library deps unicode-width + thiserror; CLI behind the `cli` feature (clap, clap_complete, anyhow, notify, serde_json).

## Commands

```bash
cargo build
cargo test                                      # invariants over tests/fixtures in all four directions
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
cargo insta review
cargo run -- check --strict
cargo run -- examples/simple.mmd                # render once
cargo run -- --watch examples/architecture.mmd  # redraw on every save
```

Release: automatic. main only takes PRs with green CI; each merge updates the release-plz PR (version + CHANGELOG from conventional commits); its CI needs a maintainer's "Approve workflows to run" click, and merging it publishes to crates.io, tags `vX.Y.Z` and dispatches cargo-dist (`release.yml`) for binaries, installers and the Homebrew formula. Commit messages must be conventional (`feat:`, `fix:`, `chore:` …). Run `dist generate` after editing `dist-workspace.toml`.

## Code map

`src/lib.rs` wires the pipeline: `parser` → `layout/` (`acyclic` → `rank` → `order` → `size` → `position`) → `route` → `render` on top of `canvas`. `src/main.rs` dispatches catalog setup, validation, listing, diffing and rendering. The binary-only `onboard` module handles the first-run menu, agent detection and launch. The README's "The renderer" explains each layout stage. The binary-only `src/browse/` TUI (behind the default-on `tui` feature) handles interactive browsing with pure app state, layout, widgets and ANSI conversion.

`anchors` (anchor syntax, matcher, file walk), `markdown` (mermaid blocks in .md), `diff` (union diagram + marks), `mermaid` (Mermaid writer), `flows` (catalog, compare, report), `git` (repository paths, ignore checks, merge base and files at a commit via the git binary), `coverage` (code classification, repository coverage and uncovered changes), `check` (catalog validation), `skill` (embedded casemap skill, source `skills/casemap/SKILL.md`).

`action.yml` is the composite GitHub Action for PR flow comments and job summaries. `.github/workflows/flows.yml` uses the local Action; `move-action-tag.yml` moves its `v0` tag after cargo-dist announces a `v0.*` release.
