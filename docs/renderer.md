# Rendering any Mermaid file

Mermaid flowcharts render as clean, readable terminal diagrams. Keep a diagram
open in a side pane while you (or your agent) work on the code:

```sh
casemap docs/architecture.mmd           # render once
casemap --watch docs/architecture.mmd   # redraw on every save
cat flow.mmd | casemap --dir=LR          # stdin, override direction
```

The source of truth is plain Mermaid in your repo, so diagrams diff, review
and version like code. The layout engine is where the work went, because
that's where terminal diagram tools usually fall apart.

In watch mode an invalid edit keeps the last good diagram on screen with the
parse error underneath, so a half-written change from an agent doesn't blank
the pane.

Supported Mermaid: `flowchart`/`graph` with TD/TB/BT/LR/RL; shapes `[]`,
`()`, `([])`, `[[]]`, `[()]`, `(())`, `{}`, `{{}}`; quoted labels and
`<br>`; chains (`a --> b --> c`); `&` fan-out; solid `-->`, dotted `-.->`,
thick `==>`, open `---`, bidirectional `<-->`; labels as `-->|x|` or
`-- x -->`; nested `subgraph … end`. Styling statements (`classDef`,
`style`, `linkStyle`, `click`) are accepted and ignored.

`casemap FILE.md` renders every Mermaid block under its nearest heading, and `casemap --watch FILE.md` redraws them on save.

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
let options = casemap::Options::default();
let text = casemap::render_str(source, &options)?;
let document = casemap::render_markdown(markdown, &options)?;

let old = casemap::parser::parse(old_source)?;
let new = casemap::parser::parse(new_source)?;
let changes = casemap::diff::diff(&old, &new);
let summary = changes.summary();
let drawing = casemap::render_diagram(&changes.union, &options);
let mermaid = casemap::mermaid::write(&changes.union);
```

For the library only, disable the default CLI feature:

```toml
casemap = { version = "0.6", default-features = false }
```
