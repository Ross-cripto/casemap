# Changelog

## 0.5.0

The first packaged release: strata is now the living map of your app's use cases. Each use case is one Mermaid flowchart in `docs/flows/`, your coding agent keeps them current, and strata checks, renders and diffs them.

- `strata init` creates the catalog and installs the `strata-flows` skill for Claude Code, Codex and other agents (`npx skills add Ross-cripto/strata` also works).
- `strata check` validates every flow with file and line, as text or JSON, and fails CI on errors (`--strict` also fails on warnings).
- `%% @code` anchors tie steps to the code that implements them. `strata which` maps files to flows, and `strata diff` flags flows that may be stale.
- `strata browse` explores the catalog in a full-screen terminal browser, with status markers, pannable diagrams, search, a diff view, a problems and code panel, and live reload. Running `strata` with no arguments in a terminal, in a repository with a catalog, opens it.
- Install with Homebrew (`brew install Ross-cripto/tap/strata`), the shell or PowerShell installer, or `cargo install`.
