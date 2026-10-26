# Changelog

## [0.5.0] - 2026-10-26

The first packaged release of casemap (developed as strata until now): the living map of your app's use cases. Each use case is one Mermaid flowchart in `docs/flows/`, your coding agent keeps them current, and casemap checks, renders and diffs them.

- `casemap init` creates the catalog and installs the `casemap` skill for Claude Code, Codex and other agents (`npx skills add Ross-cripto/casemap` also works).
- `casemap check` validates every flow with file and line, as text or JSON, and fails CI on errors (`--strict` also fails on warnings).
- `%% @code` anchors tie steps to the code that implements them. `casemap which` maps files to flows, and `casemap diff` flags flows that may be stale.
- `casemap browse` explores the catalog in a full-screen terminal browser, with status markers, pannable diagrams, search, a diff view, a problems and code panel, and live reload. Running `casemap` with no arguments in a terminal, in a repository with a catalog, opens it.
- Install with Homebrew (`brew install Ross-cripto/tap/casemap`), the shell or PowerShell installer, or `cargo install`.
