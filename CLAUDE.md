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

Release: bump `version` and `CHANGELOG.md`, merge, then push a `vX.Y.Z` tag; cargo-dist (`dist-workspace.toml`, `.github/workflows/release.yml`) builds and publishes it, and the tap `Ross-cripto/homebrew-tap` syncs the formula. Run `dist generate` after editing the dist config.

## Code map

`src/lib.rs` wires the pipeline: `parser` → `layout/` (`acyclic` → `rank` → `order` → `size` → `position`) → `route` → `render` on top of `canvas`. `src/main.rs` dispatches catalog setup, validation, listing, diffing and rendering. The README's "The renderer" explains each layout stage. The binary-only `src/browse/` TUI (behind the default-on `tui` feature) handles interactive browsing with pure app state, layout, widgets and ANSI conversion.

`anchors` (anchor syntax, matcher, file walk), `markdown` (mermaid blocks in .md), `diff` (union diagram + marks), `flows` (catalog, compare, report), `git` (repository paths, ignore checks, merge base and files at a commit via the git binary), `check` (catalog validation), `skill` (embedded casemap skill, source `skills/casemap/SKILL.md`).
