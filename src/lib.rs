//! Casemap checks, renders and compares Markdown use-case flows.
//!
//! The image pipeline parses the Mermaid flowchart, lays out its cards and
//! connections, and renders SVG. The CLI can export the SVG, rasterize it
//! to PNG, or display the image through a terminal graphics protocol.

pub mod anchors;
pub mod check;
pub mod coverage;
pub mod diff;
pub mod flow_image;
pub mod flows;
pub mod git;
pub mod icons;
pub mod map;
pub mod markdown;
pub mod mermaid;
pub mod model;
pub mod parser;
pub mod paths;
pub mod skill;
mod terminal_text;

pub use model::{Diagram, Direction, Mark};
pub use parser::ParseError;

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Plain ASCII instead of box-drawing characters.
    pub ascii: bool,
    /// ANSI colour output.
    pub color: bool,
}
