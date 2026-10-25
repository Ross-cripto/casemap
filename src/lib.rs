//! strata — Mermaid flowcharts rendered as clean, readable terminal diagrams.
//!
//! The pipeline is `parser` → `layout` (layered, cluster-aware) → `route`
//! (orthogonal edge routing) → `render` (box-drawing canvas). Each stage is a
//! plain function over plain data, so a TUI, a file watcher or an MCP server
//! can embed it directly.
//!
//! ```
//! let out = strata::render_str("graph TD\n  a[API] --> b[(DB)]", &strata::Options::default()).unwrap();
//! assert!(out.contains("API"));
//! ```

pub mod canvas;
pub mod layout;
pub mod model;
pub mod parser;
pub mod render;
pub mod route;

pub use model::{Diagram, Direction};
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
