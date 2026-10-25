//! strata — Mermaid flowcharts rendered as clean, readable terminal diagrams.
//!
//! The pipeline is `parser` → `layout` (layered, cluster-aware) → `route`
//! (orthogonal edge routing) → `render` (box-drawing canvas). Each stage is a
//! plain function over plain data, so a TUI, a file watcher or an MCP server
//! can embed it directly.
//! A flow catalog records use cases in Markdown and compares their diagrams with git.
//!
//! ```
//! let out = strata::render_str("graph TD\n  a[API] --> b[(DB)]", &strata::Options::default()).unwrap();
//! assert!(out.contains("API"));
//! ```

pub mod canvas;
pub mod diff;
pub mod flows;
pub mod git;
pub mod layout;
pub mod markdown;
pub mod model;
pub mod parser;
pub mod paths;
pub mod render;
pub mod route;

pub use model::{Diagram, Direction, Mark};
pub use parser::ParseError;

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Plain ASCII instead of box-drawing characters.
    pub ascii: bool,
    /// ANSI colour output.
    pub color: bool,
    /// Override the direction declared in the source.
    pub direction: Option<Direction>,
}

/// Lays out and draws an already-parsed diagram.
pub fn render_diagram(diagram: &Diagram, opts: &Options) -> String {
    let mut d = diagram.clone();
    if let Some(dir) = opts.direction {
        d.direction = dir;
    }
    d.fold_self_loops();
    let lay = layout::layout(&d);
    let rt = route::route(&d, &lay);
    render::render(
        &d,
        &lay,
        &rt,
        &render::Options {
            ascii: opts.ascii,
            color: opts.color,
        },
    )
}

/// Parses Mermaid flowchart source and renders it.
pub fn render_str(source: &str, opts: &Options) -> Result<String, ParseError> {
    let d = parser::parse(source)?;
    Ok(render_diagram(&d, opts))
}

/// Renders every mermaid block under its nearest heading; error lines refer to the document.
pub fn render_markdown(md: &str, opts: &Options) -> Result<String, ParseError> {
    let mut rendered = Vec::new();
    for block in markdown::mermaid_blocks(md) {
        let diagram = parser::parse(&block.source).map_err(|error| ParseError {
            line: block.line + error.line,
            message: error.message,
        })?;
        let mut output = String::new();
        if let Some(heading) = block.heading {
            output.push_str(&render::bold_line(&heading, opts.color));
        }
        output.push_str(&render_diagram(&diagram, opts));
        rendered.push(output);
    }
    Ok(rendered.join("\n"))
}
