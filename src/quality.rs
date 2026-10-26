//! Measurements of layout quality for regression comparisons, not a stable API.
//!
//! Crossings use routing geometry in abstract rank/order coordinates. Channels
//! are compressed to track indices, with their boundaries outside all tracks.
//! This preserves perpendicular intersections: horizontal runs exist only on
//! channel tracks, never inside layers, including layers carrying dummy chains.
//! Screen rotation and reflection therefore need no special crossing rules.
//! In each channel an edge has one segment in one group, so different groups
//! necessarily represent different diagram edges. Intersections are deduplicated
//! by cell, and contacts at bends or within the same fan-out bus are excluded.

use unicode_width::UnicodeWidthStr;

use crate::layout::{self, ItemKind, Layout};
use crate::model::{Diagram, Direction, Shape};
use crate::route::{self, Routing};
use crate::{Options, render_diagram};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    /// Unicode display width of the longest line rendered with default options.
    pub width: usize,
    /// Number of lines rendered with default options and the diagram's direction.
    pub height: usize,
    /// Distinct cells where different diagram edges pass straight through each
    /// other perpendicularly, excluding junctions within one fan-out group.
    pub crossings: usize,
    /// Total 90-degree direction changes along drawn paths, summed over edges.
    pub bends: usize,
    /// Sum of layers between each labelled decision edge's original source and
    /// its label, with zero for a label in the first channel after the decision.
    pub label_gap: usize,
    /// Maximum decision-edge label gap, or zero if there are no such labels.
    pub max_label_gap: usize,
    /// Sum over edges of layers spanned minus one; a four-layer span adds three.
    pub extra_layers: usize,
    /// Number of true entries in `Layout.reversed`, after folding self-loops.
    pub back_edges: usize,
    /// Rendered arrowheads pointing against the diagram's flow direction.
    pub upstream_arrows: usize,
}

/// Uses the rendering pipeline's self-loop folding, direction, Unicode and
/// colour-free defaults, without changing the supplied diagram.
pub fn measure(diagram: &Diagram) -> Metrics {
    let rendered = render_diagram(diagram, &Options::default());
    let mut prepared = diagram.clone();
    prepared.fold_self_loops();
    let layout = layout::layout(&prepared);
    let routing = route::route(&prepared, &layout);
    let mut metrics = Metrics {
        width: rendered
            .lines()
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0),
        height: rendered.lines().count(),
        crossings: route::count_crossings(&routing, &layout),
        bends: count_bends(&routing),
        back_edges: layout.reversed.iter().filter(|&&reversed| reversed).count(),
        upstream_arrows: count_upstream_arrows(&rendered, diagram.direction, false),
        ..Metrics::default()
    };
    measure_layers(&prepared, &layout, &mut metrics);
    metrics
}

/// Counts the visible glyphs so routing flags cannot hide missing arrowheads.
fn count_upstream_arrows(rendered: &str, direction: Direction, ascii: bool) -> usize {
    let arrow = match (direction, ascii) {
        (Direction::TopDown, false) => '▲',
        (Direction::BottomUp, false) => '▼',
        (Direction::LeftRight, false) => '◀',
        (Direction::RightLeft, false) => '▶',
        (Direction::TopDown, true) => '^',
        (Direction::BottomUp, true) => 'v',
        (Direction::LeftRight, true) => '<',
        (Direction::RightLeft, true) => '>',
    };
    rendered.chars().filter(|&glyph| glyph == arrow).count()
}

fn measure_layers(diagram: &Diagram, layout: &Layout, metrics: &mut Metrics) {
    let mut node_ranks = vec![0; diagram.nodes.len()];
    let mut edge_spans = vec![0usize; diagram.edges.len()];
    for item in &layout.items {
        if let ItemKind::Node(node) = item.kind {
            node_ranks[node] = item.rank;
        }
    }
    for segment in &layout.segs {
        edge_spans[segment.edge] += 1;
    }
    metrics.extra_layers = edge_spans.iter().map(|span| span.saturating_sub(1)).sum();
    for (index, item) in layout.items.iter().enumerate() {
        if !matches!(item.kind, ItemKind::Label(_)) {
            continue;
        }
        let Some(&incoming) = layout.in_segs[index].first() else {
            continue;
        };
        let edge = &diagram.edges[layout.segs[incoming].edge];
        if diagram.nodes[edge.from].shape != Shape::Diamond {
            continue;
        }
        let gap = node_ranks[edge.from].abs_diff(item.rank).saturating_sub(1);
        metrics.label_gap += gap;
        metrics.max_label_gap = metrics.max_label_gap.max(gap);
    }
}

fn count_bends(routing: &Routing) -> usize {
    // Dummy and label items join at their centre without turning. A displaced
    // destination therefore contributes exactly two turns in its channel.
    routing
        .groups
        .iter()
        .map(|group| {
            group
                .dests
                .iter()
                .filter(|dest| dest.o != group.src_o)
                .count()
                * 2
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::count_upstream_arrows;
    use crate::{Direction, Options, parser, render_diagram};

    #[test]
    fn upstream_arrows_follow_direction_and_character_set() {
        let mut diagram = parser::parse("graph TD\n A <--> B").unwrap();
        for direction in [
            Direction::TopDown,
            Direction::BottomUp,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            diagram.direction = direction;
            for ascii in [false, true] {
                let rendered = render_diagram(
                    &diagram,
                    &Options {
                        ascii,
                        ..Options::default()
                    },
                );
                assert_eq!(
                    count_upstream_arrows(&rendered, direction, ascii),
                    1,
                    "{direction:?} ascii={ascii}\n{rendered}"
                );
            }
        }
    }
}
