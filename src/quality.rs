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

use std::collections::BTreeSet;

use unicode_width::UnicodeWidthStr;

use crate::layout::{self, ItemKind, Layout};
use crate::model::{Diagram, Shape};
use crate::route::{self, Group, Routing};
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
        crossings: count_crossings(&routing),
        bends: count_bends(&routing),
        back_edges: layout.reversed.iter().filter(|&&reversed| reversed).count(),
        ..Metrics::default()
    };
    measure_layers(&prepared, &layout, &mut metrics);
    metrics
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

fn count_crossings(routing: &Routing) -> usize {
    let mut crossings = BTreeSet::new();
    for (horizontal_index, horizontal) in routing.groups.iter().enumerate() {
        let Some(track) = horizontal.track else {
            continue;
        };
        for (vertical_index, vertical) in routing.groups.iter().enumerate() {
            if horizontal_index == vertical_index || horizontal.layer != vertical.layer {
                continue;
            }
            for dest in &vertical.dests {
                if vertical.src_o == dest.o {
                    record_crossing(&mut crossings, horizontal, track, dest.o);
                    continue;
                }
                let Some(vertical_track) = vertical.track else {
                    continue;
                };
                if track < vertical_track {
                    record_crossing(&mut crossings, horizontal, track, vertical.src_o);
                }
                if track > vertical_track {
                    record_crossing(&mut crossings, horizontal, track, dest.o);
                }
            }
        }
    }
    crossings.len()
}

fn record_crossing(
    crossings: &mut BTreeSet<(usize, usize, i32)>,
    horizontal: &Group,
    track: usize,
    order: i32,
) {
    let (left, right) = horizontal.span();
    if left < order && order < right {
        crossings.insert((horizontal.layer, track, order));
    }
}
