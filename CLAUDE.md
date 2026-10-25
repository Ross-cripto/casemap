# strata

Rust CLI and library that renders Mermaid flowcharts as clean terminal diagrams, built to keep an architecture diagram open in a side pane while working on the code.

- Type: my own open source project (MIT), in `~/Code/open-source-projects/strata`.
- Stack: Rust 2024 (MSRV 1.88); library deps unicode-width + thiserror; CLI behind the `cli` feature (clap, clap_complete, anyhow, notify).

## Commands

```bash
cargo build
cargo test                                      # invariants over tests/fixtures in all four directions
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
cargo insta review
cargo run -- examples/simple.mmd                # render once
cargo run -- --watch examples/architecture.mmd  # redraw on every save
```

## Code map

`src/lib.rs` wires the pipeline: `parser` → `layout/` (`acyclic` → `rank` → `order` → `size` → `position`) → `route` → `render` on top of `canvas`. `src/main.rs` is a thin CLI over `render_str`. The README's "How the layout works" explains each stage, and its roadmap (TUI viewer, code anchors, MCP server) is the next work.

`markdown` (mermaid blocks in .md), `diff` (union diagram + marks), `flows` (catalog, compare, report), `git` (merge base and files at a commit via the git binary).
