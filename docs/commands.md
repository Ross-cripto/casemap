# Commands

## Commands

| Command | Result |
|---|---|
| `casemap init [--ci] [--force] [--agent claude\|codex]` | Create the catalog and install the agent skill; `--ci` adds the PR workflow; `--force` updates differing skills and the requested workflow; `--agent` opens that agent in a terminal |
| `casemap init --hooks` | Install the skill and merge Stop hooks for Claude Code and Codex |
| `casemap hook stop` | Check uncommitted changes for agents; block once per new warning set |
| `casemap browse [--base REF]` | Explore the catalog interactively (for humans; agents use the other commands) |
| `casemap flows [NAME]` | List the catalog, or render one use case |
| `casemap map [--layout grid\|outline]` | Draw the use-case map from the catalog's README.md; the grid when it fits the terminal (COLUMNS, else the terminal, else 80), else the outline |
| `casemap flows --format json` | List catalog metadata as JSON v1; cannot be combined with NAME |
| `casemap check [--strict] [--format text\|json]` | Validate the catalog; `--strict` also fails on warnings |
| `casemap diff [REF] [--strict] [--format text\|markdown]` | Compare with the merge base of REF and HEAD; `--format markdown` writes the PR comment and also fails on check errors (unlike text mode); `--strict` also exits 1 for possibly stale flows |
| `casemap propose NAME [--format text\|markdown]` | Preview a proposed version of a flow, read from stdin, against the current one; writes nothing; exits 1 when the proposal has check errors |
| `casemap coverage [REF] [--include P] [--exclude P] [--strict] [--format text\|json]` | Code no flow covers, by directory or changed since REF; `--strict` fails only in branch mode |
| `casemap which <file>... [--format text\|json]` | Show flows and steps anchored to files or directories |
| `casemap guide` | Print the agent skill body |
| `casemap FILE` | Render a Mermaid file, or every Mermaid block of a Markdown file |
| `casemap completions SHELL` | Generate completions for bash, zsh, fish, powershell or elvish |

## Check

`casemap check` prints each problem with its catalog path, line when available,
severity and code, followed by a summary such as `checked 3 flows: ok`.
Exit 1 means errors, warnings with `--strict`, or a command failure such as a
missing catalog. Warnings alone exit 0; an empty catalog produces one warning.

| Check code | Severity | Meaning |
|---|---|---|
| `missing-title` | error | No ATX title heading (`# Title`) |
| `missing-description` | error | No prose description after the title |
| `missing-diagram` | error | No Mermaid block |
| `not-a-flowchart` | error | The first Mermaid block does not start with a flowchart/graph header |
| `parse-error` | error | The first diagram cannot be parsed |
| `reserved-id` | error | A node id is a Mermaid keyword; rename it |
| `empty-diagram` | error | The flowchart has no steps |
| `unclosed-fence` | error | A code fence is never closed |
| `duplicate-name` | error | Another file has the same stem |
| `unreadable` | error | A flow file cannot be read as UTF-8 |
| `anchor-syntax` | error | An invalid anchor or an anchor outside the first Mermaid block |
| `anchor-unknown-step` | error | An anchor targets a node id the flowchart does not have |
| `anchor-no-match` | error | An anchor path matches no file in the repository |
| `map-unknown-flow` | error | A map link targets a file that is not a flow in the catalog |
| `map-duplicate-flow` | error | A flow appears more than once on the map |
| `unmapped-flow` | warning | A flow is missing from the use-case map |
| `map-title-mismatch` | warning | A map link's text differs from the flow's title |
| `extra-diagram` | warning | Another Mermaid block follows the first |
| `duplicate-title` | warning | An earlier file has the same title, ignoring case and surrounding whitespace |
| `empty-catalog` | warning | No flow files |
| `ignored-entry` | warning | A subdirectory or non-flow file is ignored |
| `mmd-flow` | warning | A `.mmd` file lacks the Markdown title and description format |
| `no-actor` | warning | No stadium-shaped actor or trigger |
| `unlabelled-branch` | warning | A decision has an exit without a label |
| `single-exit-decision` | warning | A decision has fewer than two exits; label both outcomes or make it a step |
| `unlabelled-step` | warning | A step's label is just its id |
| `too-many-steps` | warning | More than 15 steps |
| `too-few-steps` | warning | Fewer than 3 steps |
| `styling` | warning | Diagram styling is ignored by casemap |
| `file-name` | warning | The file stem is not kebab-case |
| `long-label` | warning | A node label line is wider than 40 display columns |
| `edge-into-actor` | warning | An edge targets a stadium actor node |

Tools can read one-line JSON from `casemap check --format json`:

```json
{"version":1,"catalog":"docs/flows","flows":3,"errors":1,"warnings":0,"problems":[{"path":"docs/flows/checkout.md","line":5,"severity":"error","code":"parse-error","message":"link has no target node"}]}
```

`path` and `line` are `null` when absent. A missing catalog exits 1, reports the
error on stderr, and leaves stdout empty, including in JSON mode.

## Listing flows

`casemap flows` and the browser list group by activity when a map exists.
Without a map, they sort by case-insensitive title, with file name as the
tie-break. `casemap flows --format json` keeps file-name order for tools and
returns catalog metadata:

```json
{"version":1,"flows":[{"name":"checkout","path":"docs/flows/checkout.md","title":"Checkout","description":"A customer pays for the cart.","has_diagram":true}]}
```

When a map exists, each JSON flow also has an `activity` key: its activity name,
or `null` for a flow not on the map. Without a map, the key is absent.

## Global flags

Global flags can appear before or after a subcommand:

| Flag | Effect |
|---|---|
| `--flows DIR` | Catalog directory, default `docs/flows`, relative to the current directory |
| `--dir TD\|BT\|LR\|RL` | Override flowchart direction; `--dir=LR` also works |
| `--ascii` | Use plain ASCII instead of box-drawing characters |
| `--color` | Force ANSI colour, including when NO_COLOR is set |
| `--no-color` | Disable ANSI colour; otherwise colour is automatic on a TTY and respects NO_COLOR |
| `-w`, `--watch` | Re-render on every file change; applies only to rendering a file |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

The command words `init`, `hook`, `flows`, `map`, `browse`, `check`, `diff`, `propose`, `which`, `coverage`, `guide` and `completions` are reserved. To render a file with one of those names, use `./diff` or
`casemap -- diff`. `help` remains a file name; use `--help` or `-h` to print help.
`init` requires a relative catalog path because the installed skill is committed.

## Known limits

- In a terminal, `casemap` with no flows opens the setup menu instead of reading stdin; pipe input or pass `casemap -` to render from stdin.
- On Windows only native `claude.exe`/`codex.exe` are launched; npm `.cmd` shims get the manual steps.
- browse restores the terminal on quit, Ctrl-C, errors and panics; a `kill` signal leaves it in the alternate screen (run `reset`).
- Anchor paths cannot contain spaces; symbol anchors and line ranges are not supported.
- Front matter needs a `key:` line immediately after the opening `---`; otherwise it is a thematic break.
- Flowcharts only, no sequence/class/ER diagrams yet.
- Self-loops are shown as a `↻` marker on the node; self-loop changes appear only in the diff summary and details.
- Node ids are the identity; changing an id reads as a removal plus an addition.
- Only the first mermaid block of a catalog flow file is compared. Prose, titles, direction and cluster membership are not diffed.
- Edges to a subgraph id (`a --> mySubgraph`) treat the id as a node.
- LR diagrams with many layers get wide; TD is the better default for big systems.
- Edges entering a cluster cross its border wherever they arrive rather than at a dedicated gate.
- README.md in a catalog is ignored, ignoring case. Hidden files are also skipped. `check` reports dangling flow symlinks as unreadable; `flows` and `diff` skip them.
