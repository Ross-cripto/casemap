# Changelog

## [0.6.0](https://github.com/Ross-cripto/casemap/compare/v0.5.1...v0.6.0) - 2026-10-26

### Added

- *(hook)* stop coding agents once when their changes leave the map out of date
- *(coverage)* show which code no use-case flow covers
- *(pr)* draw the use cases each pull request changed in a PR comment
- *(onboard)* guided first run that lets your coding agent map the repo

### Fixed

- *(check)* catch keyword ids and one-way decisions, and sharpen the skill
- *(layout)* keep the main path straight instead of drawing staircases
- *(layout)* draw loops up the side and into their target from above
- *(layout)* put edge labels next to their decision and cut crossings
- *(browse)* faster reload feedback and a status line that keeps its notes

### Other

- *(quality)* report corpus paths with '/' on every OS
- *(quality)* measure diagram layout over a real-app corpus
- *(onboard)* make the onboarding tests portable to Windows

## [0.5.1](https://github.com/Ross-cripto/casemap/compare/v0.5.0...v0.5.1) - 2026-10-26

### Other

- let a maintainer approve CI on the release PR
- release from main with release-plz and dispatched cargo-dist builds

## [0.5.0] - 2026-10-26

The first packaged release of casemap (developed as strata until now): the living map of your app's use cases. Each use case is one Mermaid flowchart in `docs/flows/`, your coding agent keeps them current, and casemap checks, renders and diffs them.

- `casemap init` creates the catalog and installs the `casemap` skill for Claude Code, Codex and other agents (`npx skills add Ross-cripto/casemap` also works).
- `casemap check` validates every flow with file and line, as text or JSON, and fails CI on errors (`--strict` also fails on warnings).
- `%% @code` anchors tie steps to the code that implements them. `casemap which` maps files to flows, and `casemap diff` flags flows that may be stale.
- `casemap browse` explores the catalog in a full-screen terminal browser, with status markers, pannable diagrams, search, a diff view, a problems and code panel, and live reload. Running `casemap` with no arguments in a terminal, in a repository with a catalog, opens it.
- Install with Homebrew (`brew install Ross-cripto/tap/casemap`), the shell or PowerShell installer, or `cargo install`.
