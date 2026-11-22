# Flow images

Render a use-case Markdown file with one Mermaid `flowchart TD` block.

```sh
casemap docs/flows/render-a-diagram.md
casemap --watch docs/flows/render-a-diagram.md
casemap export docs/flows/render-a-diagram.md --png
casemap export docs/flows/render-a-diagram.md --svg -o diagram.svg
```

Kitty, Ghostty, cmux, iTerm2 and terminals with Sixel support can show the image
inline. When graphics are unavailable, including Terminal.app and redirected
output, casemap prints the reason and the path to a temporary PNG. Each flow
reuses a PNG named from its stem and canonical path under the system's temporary
directory. On Unix, the private directory is `casemap-<uid>`. On other platforms,
it is `casemap`. In tmux,
inline graphics require `allow-passthrough`. Use `CASEMAP_IMAGE_PROTOCOL=kitty`, `iterm`,
`sixel` or `none` to override detection. For a VS Code terminal with image
support enabled, set `CASEMAP_IMAGE_PROTOCOL=sixel`.

Watch mode redraws on save. If a file becomes invalid, casemap reports the
error and keeps the last valid image. Export writes a 2× PNG by default. An SVG
export converts text to paths so it opens with the same fonts on other systems.
Geist and Geist Mono are bundled. Characters outside those fonts use installed
fonts when available.

## Reading a flow

Named Mermaid `subgraph` blocks appear as colored sections. Steps use the same
card shape, with an icon chosen by `%% @icon` or by the step type. A single
continuation goes down. Side branches start on a horizontal line to the right.
Decision labels sit beside their continuation or on their side branch, with
green for `yes`, red for `no` and the destination section's color for other
actions. Loops use **Back to** and extra paths into a step use **Go to**.
Both jumps leave the right edge of the source card and name the destination
section and, when needed, the step within it.

`casemap diff` and `casemap propose` mark new, edited and removed steps on the
image. `--side-by-side` draws the previous and proposed versions together.
GitHub PR comments use Mermaid so reviewers can read the diagram in the PR.

## Themes

Casemap loads the repository's `casemap.toml` and applies the user's settings
from `~/.config/casemap/config.toml` or `$XDG_CONFIG_HOME/casemap/config.toml`.
`--theme dark`, `--theme light` and `--bg '#111827'` override their respective
settings for one invocation.

```toml
[render]
theme = "dark"
background = "#111827"
sections = ["#a8bafa", "#78c9c1", "#f1b780"]
```

Set any of `card`, `text`, `muted`, `yes`, `no`, `added`, `changed` and `removed`
in `[render]` to customize the corresponding color. Values accept `#RGB` or
`#RRGGBB`. Section colors cycle in declaration order.

## Rust library

Read the flow and use `flow_image::svg` to generate markup. Use
`flow_image::png` for raster output or `flow_image::outlined_svg` for portable
SVG. These functions are available with the default features disabled.

```rust
use casemap::flow_image::{ImageOptions, Theme, parse_flow, png, svg};

let flow = casemap::flows::parse_flow("checkout.md", markdown);
let diagram = parse_flow(&flow)?;
let svg = svg(&flow, &diagram, &Theme::dark(), ImageOptions::default());
let bytes = png(&svg, 2.0)?;
```
