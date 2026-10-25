# strata

Mermaid flowcharts rendered as clean, readable terminal diagrams, built to
keep an architecture diagram open in a side pane while you (or your agent)
work on the code.

```
strata docs/architecture.mmd           # render once
strata --watch docs/architecture.mmd   # redraw on every save
cat flow.mmd | strata --dir LR         # stdin, override direction
```

The source of truth is plain Mermaid in your repo, so diagrams diff, review
and version like code. The layout engine is where the work went, because
that's where terminal diagram tools usually fall apart.

## Usage

```
strata [OPTIONS] [FILE]

  -w, --watch           Re-render whenever FILE changes (Ctrl-C to quit)
  --dir <TD|BT|LR|RL>   Override the flowchart direction
  --ascii               Plain ASCII instead of box-drawing characters
  --color / --no-color  Force or disable ANSI colour (auto on a TTY; respects NO_COLOR)
```

In watch mode an invalid edit keeps the last good diagram on screen with the
parse error underneath, so a half-written change from an agent doesn't blank
the pane.

Supported Mermaid: `flowchart`/`graph` with TD/TB/BT/LR/RL; shapes `[]`,
`()`, `([])`, `[[]]`, `[()]`, `(())`, `{}`, `{{}}`; quoted labels and
`<br>`; chains (`a --> b --> c`); `&` fan-out; solid `-->`, dotted `-.->`,
thick `==>`, open `---`, bidirectional `<-->`; labels as `-->|x|` or
`-- x -->`; nested `subgraph … end`. Styling statements (`classDef`,
`style`, `linkStyle`, `click`) are accepted and ignored.

## How the layout works

```
parse → break cycles → rank → split long edges → order → size → position → route → draw
```

| Stage | What it does | Why it matters in a terminal |
|---|---|---|
| Break cycles | Adds edges in declaration order and reverses only the ones that close a cycle | The edge drawn "backwards" is the one you wrote as feedback (`retry`), not an arbitrary one |
| Rank | Longest path, then median balancing on edge length | Sources and sinks sit next to their neighbours instead of stranded at the top or bottom |
| Split long edges | Edges spanning several layers get pass-through points; labelled edges get a dedicated label row | Labels never collide with nodes or other edges |
| Order | Barycenter sweeps plus adjacent transposition, keeping each subgraph contiguous between its border chains | Fewer crossings; clusters never interleave |
| Position | Per layer, weighted least squares under spacing constraints (isotonic regression via pool-adjacent-violators), then relaxation | Siblings spread symmetrically under a parent; long edges stay straight |
| Clusters | Each side snaps to one straight line hugging its members; columns are inserted for titles | Tight boxes that never contain outsiders |
| Route | Ports placed per edge, buses for fan-out, tracks ordered so a vertical never cuts a horizontal it could avoid | Orthogonal, readable wiring; straight lines wherever geometrically possible |
| Draw | Lines stored as per-cell connection flags and resolved to glyphs at the end | Correct junctions (`┬ ┴ ├ ┼`) regardless of draw order; crossings render as hops, not fake junctions |

All coordinates are abstract (rank axis × order axis), so TD, BT, LR and RL
share one code path. Dotted edges use `┄┆`, thick use `━┃`, cluster borders
use a fainter `┈┊`, and edges crossing a cluster border interrupt the border,
not the edge.

## Library

The CLI is a thin wrapper around the crate, so a TUI or an MCP server can embed it:

```rust
let text = strata::render_str(source, &strata::Options::default())?;
```

## Tests

`cargo test` checks invariants across every fixture in all four directions:
exactly one arrowhead per arrow end, every node and edge label visible, no
dangling line ends, deterministic output, and pure-ASCII output in `--ascii` mode.

## Known limits

- Flowcharts only, no sequence/class/ER diagrams yet.
- Self-loops are shown as a `↻` marker on the node.
- Edges to a subgraph id (`a --> mySubgraph`) treat the id as a node.
- LR diagrams with many layers get wide; TD is the better default for big systems.
- Edges entering a cluster cross its border wherever they arrive rather than at a dedicated gate.

## Roadmap

1. **TUI viewer**: pan and zoom for diagrams wider than the terminal, focus a node to dim everything outside its neighbourhood, follow mode in watch.
2. **Code anchors**: `%% @anchor billing/charge_service.py::ChargeService` on a node; a watcher marks the node stale when that symbol moves or disappears.
3. **MCP server**: `add_node`, `connect`, `mark_stale`, `render` tools, so a coding agent keeps the diagram current as part of the plan → implement → review loop.
4. Sequence diagrams for request flows.
