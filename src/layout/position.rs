//! Coordinate assignment along the order axis.
//!
//! 1. Sweep layers up and down, pulling each item toward the mean centre of
//!    its neighbours, then relax toward the mean of both sides. Each layer is
//!    placed by weighted least squares under spacing constraints; long-edge
//!    pass-throughs weigh most, so long edges stay straight while parents
//!    centre over their children. Cluster borders are passive here.
//! 2. Snap each cluster side onto one straight line hugging its members.
//! 3. Sweep again with the borders fixed so outside items re-align around them.
//! 4. Straighten any long edge that can run as one vertical line.
//! 5. Keep decision centres on a dominant forward path rigid, then compact
//!    under the same spacing constraints. Action boxes align when they fit;
//!    wider boxes use port slack. Relax side branches between fixed anchors.
//! 6. Insert columns where a cluster is too narrow for its title.

use std::cmp::Reverse;

use unicode_width::UnicodeWidthStr;

use super::{Item, ItemKind, Layout};
use crate::{model::Diagram, route};

const PASS_THROUGH_WEIGHT: f64 = 100.0;

struct Spacing {
    node: i32,
    thin: i32,
    border: i32,
}

struct SpinePaths {
    successors: Vec<Option<usize>>,
    descendants: Vec<usize>,
}

struct SpineBlocks {
    rigid: Vec<usize>,
    actions: Vec<(usize, usize)>,
}

impl Spacing {
    fn gap(&self, a: &Item, b: &Item) -> i32 {
        if a.is_border() || b.is_border() {
            self.border
        } else if a.is_node() && b.is_node() {
            self.node
        } else {
            self.thin
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Follow the layer above, top to bottom.
    Down,
    /// Follow the layer below, bottom to top.
    Up,
    /// Balance between both neighbouring layers, top to bottom.
    BothDown,
    /// Balance between both neighbouring layers, bottom to top.
    BothUp,
}

pub fn assign(lay: &mut Layout, d: &Diagram) {
    const LOOP_LANE_GAP: i32 = 1;
    const FORWARD_LANE_GAP: i32 = 2;
    let has_loops = lay.reversed.iter().any(|&reversed| reversed);
    let sp = if lay.direction.is_vertical() {
        Spacing {
            node: 4,
            // Returns add a lane beside their target, so thin items use tighter clearance.
            thin: if has_loops {
                LOOP_LANE_GAP
            } else {
                FORWARD_LANE_GAP
            },
            border: 2,
        }
    } else {
        Spacing {
            node: 1,
            thin: 1,
            border: 1,
        }
    };
    let n = lay.items.len();
    let mut up = vec![Vec::new(); n];
    let mut down = vec![Vec::new(); n];
    for s in &lay.segs {
        if matches!(lay.items[s.from].kind, ItemKind::LoopTurn) && s.last && !s.first {
            continue;
        }
        down[s.from].push(s.to);
        up[s.to].push(s.from);
    }

    // Initial left-packed placement.
    for l in 0..lay.layers.len() {
        let mut x = 0;
        for i in 0..lay.layers[l].len() {
            let it = lay.layers[l][i];
            if i > 0 {
                let prev = lay.layers[l][i - 1];
                x += sp.gap(&lay.items[prev], &lay.items[it]);
            }
            lay.items[it].pos = x;
            x += lay.items[it].ext_o;
        }
    }

    for _ in 0..8 {
        sweep(lay, &sp, &up, &down, Mode::Down, false);
        sweep(lay, &sp, &up, &down, Mode::Up, false);
    }
    // Relax: every item settles at the mean of all its neighbours, so parents
    // centre over children and joins centre under their sources.
    for _ in 0..4 {
        sweep(lay, &sp, &up, &down, Mode::BothDown, false);
        sweep(lay, &sp, &up, &down, Mode::BothUp, false);
    }

    let has_clusters = lay.items.iter().any(Item::is_border);
    if has_clusters {
        hug_borders(lay, &sp);
        for _ in 0..4 {
            sweep(lay, &sp, &up, &down, Mode::Down, true);
            sweep(lay, &sp, &up, &down, Mode::Up, true);
        }
        hug_borders(lay, &sp);
        sweep(lay, &sp, &up, &down, Mode::Up, true);
    }
    straighten_long_edges(lay, &sp);
    snap_chains(lay, &sp);
    align_spines(lay, d, &sp, &up, &down);
    if lay.direction.is_vertical() {
        widen_for_titles(lay, d);
    }
    normalize(lay);
}

/// A continuation carrying more descendants takes priority over terminal
/// branches so decisions do not centre over each successive pair of exits.
fn spine_successors(layout: &Layout) -> SpinePaths {
    let mut successors = vec![None; layout.items.len()];
    let descendants = descendant_counts(layout);
    let mut next_nodes = vec![usize::MAX; layout.items.len()];
    for layer in layout.layers.iter().rev() {
        for &item in layer {
            let mut best = None;
            for &segment_index in &layout.out_segs[item] {
                let segment = &layout.segs[segment_index];
                if layout.reversed[segment.edge] {
                    continue;
                }
                let child = segment.to;
                let priority = (
                    descendants[child],
                    Reverse(next_nodes[child]),
                    Reverse(child),
                );
                if best.is_none_or(|previous| priority > previous) {
                    best = Some(priority);
                    successors[item] = Some(child);
                }
            }
            let continuation = successors[item].map_or(0, |child| descendants[child]);
            if continuation == 1 && layout.out_segs[item].len() > 1 {
                successors[item] = None;
            }
            next_nodes[item] = match layout.items[item].kind {
                ItemKind::Node(node) => node,
                _ => successors[item].map_or(usize::MAX, |child| next_nodes[child]),
            };
        }
    }
    SpinePaths {
        successors,
        descendants,
    }
}

/// Count shared descendants once so a reconverging branch cannot outweigh a
/// larger continuation merely by having more routes to the same nodes.
pub(super) fn descendant_counts(layout: &Layout) -> Vec<usize> {
    let mut descendants = vec![vec![false; layout.items.len()]; layout.items.len()];
    for layer in layout.layers.iter().rev() {
        for &item in layer {
            descendants[item][item] = layout.items[item].is_node();
            for &segment_index in &layout.out_segs[item] {
                let segment = &layout.segs[segment_index];
                if layout.reversed[segment.edge] {
                    continue;
                }
                let child_descendants = descendants[segment.to].clone();
                for (descendant, reachable) in child_descendants.into_iter().enumerate() {
                    descendants[item][descendant] |= reachable;
                }
            }
        }
    }
    descendants
        .iter()
        .map(|reachable| reachable.iter().filter(|&&present| present).count())
        .collect()
}

/// Border chains must remain rigid too: independently moving one layer could
/// leave cluster members outside their rendered rectangle.
fn border_blocks(layout: &Layout) -> Vec<usize> {
    let mut blocks: Vec<usize> = (0..layout.items.len()).collect();
    let mut borders = Vec::new();
    for (item_index, item) in layout.items.iter().enumerate() {
        let ItemKind::Border { cluster, left } = item.kind else {
            continue;
        };
        if let Some(&(_, _, block)) = borders
            .iter()
            .find(|&&(other_cluster, other_left, _)| other_cluster == cluster && other_left == left)
        {
            blocks[item_index] = block;
        } else {
            borders.push((cluster, left, item_index));
        }
    }
    blocks
}

/// Optional action-box alignments yield to return lanes when they would add a
/// routed crossing. Decision and pass-through blocks remain rigid in every trial.
fn align_spines(
    layout: &mut Layout,
    diagram: &Diagram,
    spacing: &Spacing,
    up: &[Vec<usize>],
    down: &[Vec<usize>],
) {
    let Some(SpineBlocks { mut rigid, actions }) = build_spine_blocks(layout, spacing) else {
        return;
    };
    let unfitted = rigid.clone();
    fit_action_boxes(layout, spacing, &mut rigid, &actions);
    place_spine_blocks(layout, spacing, &rigid, &actions, (up, down), true);
    let mut best_items = layout.items.clone();
    let routing = route::route(diagram, layout);
    let mut best_crossings = route::count_crossings(&routing, layout);
    for (alternative, pinned) in [(&unfitted, true), (&unfitted, false), (&rigid, false)] {
        if best_crossings == 0 {
            break;
        }
        place_spine_blocks(layout, spacing, alternative, &actions, (up, down), pinned);
        let routing = route::route(diagram, layout);
        let crossings = route::count_crossings(&routing, layout);
        if crossings < best_crossings {
            best_crossings = crossings;
            best_items = layout.items.clone();
        }
    }
    layout.items = best_items;
}

/// Rejects alignments that would reverse an existing layer order. Main paths
/// are considered first; shorter branches retain separate blocks at joins.
fn build_spine_blocks(layout: &Layout, spacing: &Spacing) -> Option<SpineBlocks> {
    let SpinePaths {
        successors,
        descendants,
    } = spine_successors(layout);
    let has_continuation = layout.items.iter().enumerate().any(|(item_index, item)| {
        item.is_node() && layout.out_segs[item_index].len() > 1 && successors[item_index].is_some()
    });
    if !has_continuation {
        return None;
    }
    let mut roots: Vec<usize> = (0..layout.items.len()).collect();
    roots.sort_by_key(|&item| (Reverse(descendants[item]), item));
    let mut blocks = border_blocks(layout);
    let mut visited = vec![false; layout.items.len()];
    let mut flexible = Vec::new();
    for root in roots {
        if visited[root] || !layout.items[root].is_node() {
            continue;
        }
        mark_component(layout, root, &mut visited);
        let mut path = vec![root];
        while let Some(child) = successors[*path.last().expect("spine has a root")] {
            path.push(child);
        }
        let Some(&anchor) = path.iter().find(|&&item| layout.out_segs[item].len() > 1) else {
            continue;
        };
        for item in path {
            if layout.items[item].is_node() && layout.out_segs[item].len() <= 1 {
                flexible.push((item, anchor));
                continue;
            }
            let candidate = merge_blocks(&blocks, item, anchor);
            if compact_blocks(layout, spacing, &candidate).is_none() {
                break;
            }
            blocks = candidate;
        }
    }
    Some(SpineBlocks {
        rigid: blocks,
        actions: flexible,
    })
}

/// Keep border and spine blocks immovable while the existing least-squares
/// placement straightens free branches inside the compact envelope.
fn place_spine_blocks(
    layout: &mut Layout,
    spacing: &Spacing,
    blocks: &[usize],
    flexible: &[(usize, usize)],
    neighbours: (&[Vec<usize>], &[Vec<usize>]),
    pin_actions: bool,
) {
    if let Some(centers) = compact_blocks(layout, spacing, blocks) {
        for (item_index, item) in layout.items.iter_mut().enumerate() {
            item.pos = centers[blocks[item_index]] - (item.ext_o - 1) / 2;
        }
        center_flexible_items(layout, spacing, flexible);
        let pinned = if pin_actions { flexible } else { &[] };
        relax_branches(layout, spacing, blocks, pinned, neighbours.0, neighbours.1);
    }
}

fn merge_blocks(blocks: &[usize], item: usize, anchor: usize) -> Vec<usize> {
    let mut merged = blocks.to_vec();
    let previous = blocks[item];
    for block in &mut merged {
        if *block == previous {
            *block = blocks[anchor];
        }
    }
    merged
}

/// Prefer exact centres for action boxes whenever the alignment fits inside
/// the compact envelope; reserve port slack for boxes that would widen it.
fn fit_action_boxes(
    layout: &Layout,
    spacing: &Spacing,
    blocks: &mut Vec<usize>,
    flexible: &[(usize, usize)],
) {
    let Some(centers) = compact_blocks(layout, spacing, blocks) else {
        return;
    };
    let extent = block_extent(layout, blocks, &centers);
    for &(item, anchor) in flexible {
        let candidate = merge_blocks(blocks, item, anchor);
        if let Some(centers) = compact_blocks(layout, spacing, &candidate) {
            if block_extent(layout, &candidate, &centers) <= extent {
                *blocks = candidate;
            }
        }
    }
}

fn block_extent(layout: &Layout, blocks: &[usize], centers: &[i32]) -> i32 {
    layout
        .items
        .iter()
        .enumerate()
        .map(|(item_index, item)| centers[blocks[item_index]] + item.ext_o / 2 + 1)
        .max()
        .unwrap_or(0)
}

/// Reuse neighbour-mean placement between rigid anchors so side branches and
/// pass-throughs can straighten without moving the spine or widening its bounds.
fn relax_branches(
    layout: &mut Layout,
    spacing: &Spacing,
    blocks: &[usize],
    flexible: &[(usize, usize)],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
) {
    const RELAXATION_PASSES: usize = 8;
    let mut members = vec![0; layout.items.len()];
    for &block in blocks {
        members[block] += 1;
    }
    let mut fixed: Vec<bool> = layout
        .items
        .iter()
        .enumerate()
        .map(|(item_index, item)| members[blocks[item_index]] > 1 || item.is_border())
        .collect();
    for &(item_index, _) in flexible {
        fixed[item_index] = true;
    }
    let extent = layout
        .items
        .iter()
        .map(|item| item.pos + item.ext_o)
        .max()
        .unwrap_or(0);
    for _ in 0..RELAXATION_PASSES {
        for rank in 0..layout.layers.len() {
            relax_layer(layout, spacing, rank, &fixed, up, extent);
        }
        for rank in (0..layout.layers.len()).rev() {
            relax_layer(layout, spacing, rank, &fixed, down, extent);
        }
    }
}

/// Fixed spine items and cluster borders divide a layer into independent runs;
/// each run must fit between its anchors and inside the compact envelope.
fn relax_layer(
    layout: &mut Layout,
    spacing: &Spacing,
    rank: usize,
    fixed: &[bool],
    neighbours: &[Vec<usize>],
    extent: i32,
) {
    let layer = layout.layers[rank].clone();
    let mut start = 0;
    for end in 0..=layer.len() {
        if end < layer.len() && !fixed[layer[end]] {
            continue;
        }
        if start < end {
            let lower = if start == 0 {
                0
            } else {
                let previous = &layout.items[layer[start - 1]];
                previous.pos + previous.ext_o + spacing.gap(previous, &layout.items[layer[start]])
            };
            let upper = if end == layer.len() {
                extent
            } else {
                let next = &layout.items[layer[end]];
                next.pos - spacing.gap(&layout.items[layer[end - 1]], next)
            };
            place_bounded_run(
                layout,
                spacing,
                &layer[start..end],
                neighbours,
                (lower, upper),
            );
        }
        start = end + 1;
    }
}

/// Clamping the translated isotonic targets to one interval preserves the
/// existing least-squares compaction while enforcing both ends of a free run.
fn place_bounded_run(
    layout: &mut Layout,
    spacing: &Spacing,
    run: &[usize],
    neighbours: &[Vec<usize>],
    bounds: (i32, i32),
) {
    let mut offsets = Vec::with_capacity(run.len());
    let mut width = 0;
    for (slot, &item_index) in run.iter().enumerate() {
        let item = &layout.items[item_index];
        if slot > 0 {
            width += spacing.gap(&layout.items[run[slot - 1]], item);
        }
        offsets.push(width);
        width += item.ext_o;
    }
    let mut desired = Vec::with_capacity(run.len());
    let mut weights = Vec::with_capacity(run.len());
    for (slot, &item_index) in run.iter().enumerate() {
        let item = &layout.items[item_index];
        let adjacent = &neighbours[item_index];
        let target = if adjacent.is_empty() {
            item.center()
        } else {
            let total: i64 = adjacent
                .iter()
                .map(|&neighbour| i64::from(layout.items[neighbour].center()))
                .sum();
            (total as f64 / adjacent.len() as f64).round() as i32
        };
        let translated = target - (item.ext_o - 1) / 2 - offsets[slot];
        let bounded = translated.clamp(bounds.0, bounds.1 - width);
        desired.push(Some(bounded + offsets[slot] + (item.ext_o - 1) / 2));
        weights.push(
            if matches!(item.kind, ItemKind::Dummy | ItemKind::Label(_)) {
                PASS_THROUGH_WEIGHT
            } else {
                adjacent.len().max(1) as f64
            },
        );
    }
    place(layout, spacing, run, &desired, &weights);
}

/// A wide action box may use an off-centre port without bending the main path.
/// Keep it inside the compact envelope rather than widening every spine layer.
fn center_flexible_items(layout: &mut Layout, spacing: &Spacing, flexible: &[(usize, usize)]) {
    let extent = layout
        .items
        .iter()
        .map(|item| item.pos + item.ext_o)
        .max()
        .unwrap_or(0);
    for &(item_index, anchor) in flexible {
        let item = &layout.items[item_index];
        let layer = &layout.layers[item.rank];
        let slot = layer
            .iter()
            .position(|&member| member == item_index)
            .unwrap();
        let lower = if slot == 0 {
            0
        } else {
            let previous = &layout.items[layer[slot - 1]];
            previous.pos + previous.ext_o + spacing.gap(previous, item)
        };
        let upper = if slot + 1 == layer.len() {
            extent - item.ext_o
        } else {
            let next = &layout.items[layer[slot + 1]];
            next.pos - spacing.gap(item, next) - item.ext_o
        };
        let desired = layout.items[anchor].center() - (item.ext_o - 1) / 2;
        layout.items[item_index].pos = desired.clamp(lower, upper);
    }
}

/// One spine per connected component leaves shorter branches free to compact
/// around it instead of imposing competing rigid paths through the same join.
fn mark_component(layout: &Layout, root: usize, visited: &mut [bool]) {
    let mut pending = vec![root];
    while let Some(item) = pending.pop() {
        if visited[item] {
            continue;
        }
        visited[item] = true;
        for &segment_index in layout.out_segs[item].iter().chain(&layout.in_segs[item]) {
            let segment = &layout.segs[segment_index];
            let neighbour = if segment.from == item {
                segment.to
            } else {
                segment.from
            };
            if !visited[neighbour] {
                pending.push(neighbour);
            }
        }
    }
}

/// Rigid alignments turn layer spacing into difference constraints between
/// blocks. A topological pass packs them without stretching any spine.
fn compact_blocks(layout: &Layout, spacing: &Spacing, blocks: &[usize]) -> Option<Vec<i32>> {
    let count = layout.items.len();
    let mut outgoing = vec![Vec::new(); count];
    let mut incoming = vec![0usize; count];
    let mut centers = vec![0; count];
    for (item_index, item) in layout.items.iter().enumerate() {
        centers[blocks[item_index]] = centers[blocks[item_index]].max((item.ext_o - 1) / 2);
    }
    for layer in &layout.layers {
        for pair in layer.windows(2) {
            let left = pair[0];
            let right = pair[1];
            let left_block = blocks[left];
            let right_block = blocks[right];
            if left_block == right_block {
                return None;
            }
            let distance = layout.items[left].ext_o / 2
                + spacing.gap(&layout.items[left], &layout.items[right])
                + (layout.items[right].ext_o - 1) / 2
                + 1;
            outgoing[left_block].push((right_block, distance));
            incoming[right_block] += 1;
        }
    }
    let mut ready: Vec<usize> = (0..count).filter(|&block| incoming[block] == 0).collect();
    let mut visited = 0;
    while let Some(block) = ready.pop() {
        visited += 1;
        for &(next, distance) in &outgoing[block] {
            centers[next] = centers[next].max(centers[block] + distance);
            incoming[next] -= 1;
            if incoming[next] == 0 {
                ready.push(next);
            }
        }
    }
    if visited == count {
        Some(centers)
    } else {
        None
    }
}

fn sweep(
    lay: &mut Layout,
    sp: &Spacing,
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    mode: Mode,
    borders_fixed: bool,
) {
    let nl = lay.layers.len();
    if nl < 2 {
        return;
    }
    let order: Vec<usize> = match mode {
        Mode::Down => (1..nl).collect(),
        Mode::Up => (0..nl - 1).rev().collect(),
        Mode::BothDown => (0..nl).collect(),
        Mode::BothUp => (0..nl).rev().collect(),
    };
    for l in order {
        let layer = lay.layers[l].clone();
        let mut desired = vec![None; layer.len()];
        let mut weight = vec![1.0f64; layer.len()];
        for (i, &it) in layer.iter().enumerate() {
            if lay.items[it].is_border() {
                if borders_fixed {
                    desired[i] = Some(lay.items[it].center());
                    weight[i] = 1e6;
                }
                continue;
            }
            let nb: Vec<usize> = match mode {
                Mode::Down => up[it].clone(),
                Mode::Up => down[it].clone(),
                Mode::BothDown | Mode::BothUp => up[it].iter().chain(&down[it]).copied().collect(),
            };
            if nb.is_empty() {
                continue;
            }
            let sum: i64 = nb.iter().map(|&w| lay.items[w].center() as i64).sum();
            desired[i] = Some((sum as f64 / nb.len() as f64).round() as i32);
            // Long-edge pass-throughs barely yield, so long edges stay straight.
            weight[i] = match lay.items[it].kind {
                ItemKind::Dummy | ItemKind::Label(_) => PASS_THROUGH_WEIGHT,
                _ => nb.len() as f64,
            };
        }
        place(lay, sp, &layer, &desired, &weight);
    }
}

/// Places one layer: minimises the weighted squared distance of every item
/// from where it wants to be, subject to keeping order and spacing.
///
/// With `offset[i]` the minimum distance of item i from the first item, the
/// spacing constraints become `y[i] <= y[i+1]` on `y = x - offset`, so this is
/// weighted isotonic regression, solved exactly by pool-adjacent-violators.
/// Items wanting the same spot therefore spread symmetrically around it.
fn place(lay: &mut Layout, sp: &Spacing, layer: &[usize], desired: &[Option<i32>], weight: &[f64]) {
    let k = layer.len();
    if k == 0 {
        return;
    }
    let mut offset = vec![0i32; k];
    for m in 1..k {
        let (a, b) = (&lay.items[layer[m - 1]], &lay.items[layer[m]]);
        offset[m] = offset[m - 1] + a.ext_o + sp.gap(a, b);
    }
    // Items without a preference stay put unless something needs their room.
    let target: Vec<f64> = (0..k)
        .map(|i| {
            let it = &lay.items[layer[i]];
            let start = desired[i].map_or(it.pos, |c| c - (it.ext_o - 1) / 2);
            (start - offset[i]) as f64
        })
        .collect();
    let w: Vec<f64> = (0..k)
        .map(|i| {
            if desired[i].is_some() {
                weight[i]
            } else {
                1e-3
            }
        })
        .collect();

    // Blocks of (total weight, weighted sum, length).
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(k);
    for i in 0..k {
        blocks.push((w[i], w[i] * target[i], 1));
        while blocks.len() >= 2 {
            let (w2, s2, n2) = blocks[blocks.len() - 1];
            let (w1, s1, n1) = blocks[blocks.len() - 2];
            if s1 / w1 <= s2 / w2 {
                break;
            }
            blocks.pop();
            let last = blocks.len() - 1;
            blocks[last] = (w1 + w2, s1 + s2, n1 + n2);
        }
    }
    let mut i = 0;
    let mut prev = i32::MIN;
    for (bw, bs, n) in blocks {
        let y = ((bs / bw).round() as i32).max(prev);
        for _ in 0..n {
            lay.items[layer[i]].pos = y + offset[i];
            i += 1;
        }
        prev = y;
    }
}

/// Puts each cluster side on one straight line as close to the members as the
/// tightest layer allows, pushing outside items out of the way.
fn hug_borders(lay: &mut Layout, sp: &Spacing) {
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for it in &lay.items {
        if let ItemKind::Border { cluster, .. } = it.kind {
            if !clusters.iter().any(|c| c.0 == cluster) {
                clusters.push((cluster, it.path.len()));
            }
        }
    }
    clusters.sort_by_key(|c| Reverse(c.1));
    for _ in 0..3 {
        for &(c, _) in &clusters {
            hug_side(lay, sp, c, true);
            hug_side(lay, sp, c, false);
        }
    }
}

fn hug_side(lay: &mut Layout, sp: &Spacing, c: usize, left: bool) {
    let me = ItemKind::Border { cluster: c, left };
    let other = ItemKind::Border {
        cluster: c,
        left: !left,
    };
    let mut chain = Vec::new();
    let mut target: Option<i32> = None;
    for (l, layer) in lay.layers.iter().enumerate() {
        let Some(idx) = layer.iter().position(|&x| lay.items[x].kind == me) else {
            continue;
        };
        chain.push((l, idx));
        let b = &lay.items[layer[idx]];
        if left {
            if let Some(&nx) = layer.get(idx + 1) {
                let nx = &lay.items[nx];
                if nx.kind != other {
                    let t = nx.pos - sp.gap(b, nx) - b.ext_o;
                    target = Some(target.map_or(t, |x| x.min(t)));
                }
            }
        } else if idx > 0 {
            let pv = &lay.items[layer[idx - 1]];
            if pv.kind != other {
                let t = pv.pos + pv.ext_o + sp.gap(pv, b);
                target = Some(target.map_or(t, |x| x.max(t)));
            }
        }
    }
    let Some(target) = target else { return };
    for (l, idx) in chain {
        let layer = lay.layers[l].clone();
        lay.items[layer[idx]].pos = target;
        if left {
            for m in (0..idx).rev() {
                let (a, nb) = (layer[m], layer[m + 1]);
                let max_p =
                    lay.items[nb].pos - sp.gap(&lay.items[a], &lay.items[nb]) - lay.items[a].ext_o;
                if lay.items[a].pos > max_p {
                    lay.items[a].pos = max_p;
                } else {
                    break;
                }
            }
        } else {
            for m in idx + 1..layer.len() {
                let (pa, a) = (layer[m - 1], layer[m]);
                let min_p =
                    lay.items[pa].pos + lay.items[pa].ext_o + sp.gap(&lay.items[pa], &lay.items[a]);
                if lay.items[a].pos < min_p {
                    lay.items[a].pos = min_p;
                } else {
                    break;
                }
            }
        }
    }
}

/// Aligns every pass-through of a long edge on one line when that fits
/// without disturbing anything else, removing small zig-zags.
fn straighten_long_edges(lay: &mut Layout, sp: &Spacing) {
    let n_edges = lay.reversed.len();
    let mut chains: Vec<Vec<usize>> = vec![Vec::new(); n_edges];
    let mut ends: Vec<(Option<usize>, Option<usize>)> = vec![(None, None); n_edges];
    for s in &lay.segs {
        if s.first && matches!(lay.items[s.from].kind, ItemKind::LoopTurn) {
            chains[s.edge].push(s.from);
        }
        if !lay.items[s.to].is_node() {
            chains[s.edge].push(s.to);
        }
        if s.first {
            ends[s.edge].0 = Some(s.from);
        }
        if s.last {
            ends[s.edge].1 = Some(s.to);
        }
    }
    let mut slot = vec![0usize; lay.items.len()];
    for layer in &lay.layers {
        for (i, &it) in layer.iter().enumerate() {
            slot[it] = i;
        }
    }
    for (e, chain) in chains.iter().enumerate() {
        if chain.is_empty() {
            continue;
        }
        let mut centers: Vec<i32> = chain.iter().map(|&i| lay.items[i].center()).collect();
        if centers.iter().all(|&c| c == centers[0]) {
            continue;
        }
        let mut candidates = Vec::new();
        if let Some(t) = ends[e].1 {
            candidates.push(lay.items[t].center());
        }
        if let Some(s) = ends[e].0 {
            candidates.push(lay.items[s].center());
        }
        centers.sort_unstable();
        candidates.push(centers[centers.len() / 2]);
        candidates.extend(centers.iter().copied());

        let fits = |x: i32| {
            chain.iter().all(|&it| {
                let item = &lay.items[it];
                let layer = &lay.layers[item.rank];
                let i = slot[it];
                let p = x - (item.ext_o - 1) / 2;
                let left_ok = i == 0 || {
                    let a = &lay.items[layer[i - 1]];
                    a.pos + a.ext_o + sp.gap(a, item) <= p
                };
                let right_ok = i + 1 >= layer.len() || {
                    let b = &lay.items[layer[i + 1]];
                    p + item.ext_o + sp.gap(item, b) <= b.pos
                };
                left_ok && right_ok
            })
        };
        if let Some(x) = candidates.into_iter().find(|&x| fits(x)) {
            for &it in chain {
                let half = (lay.items[it].ext_o - 1) / 2;
                lay.items[it].pos = x - half;
            }
        }
    }
}

/// Aligns one-to-one links (a single edge out of one item into an item with
/// a single incoming edge) when the target, or failing that the source, can
/// move without disturbing its neighbours. Removes one-cell steps along
/// simple chains, which ports cannot hide on short LR boxes.
fn snap_chains(lay: &mut Layout, sp: &Spacing) {
    let mut slot = vec![0usize; lay.items.len()];
    for layer in &lay.layers {
        for (i, &it) in layer.iter().enumerate() {
            slot[it] = i;
        }
    }
    let fits = |lay: &Layout, it: usize, center: i32| -> bool {
        let item = &lay.items[it];
        let layer = &lay.layers[item.rank];
        let i = slot[it];
        let p = center - (item.ext_o - 1) / 2;
        let left_ok = i == 0 || {
            let a = &lay.items[layer[i - 1]];
            a.pos + a.ext_o + sp.gap(a, item) <= p
        };
        let right_ok = i + 1 >= layer.len() || {
            let b = &lay.items[layer[i + 1]];
            p + item.ext_o + sp.gap(item, b) <= b.pos
        };
        left_ok && right_ok
    };
    let mut segs: Vec<usize> = (0..lay.segs.len()).collect();
    segs.sort_by_key(|&s| lay.items[lay.segs[s].from].rank);
    for s in segs {
        let (u, v) = (lay.segs[s].from, lay.segs[s].to);
        if lay.out_segs[u].len() != 1 || lay.in_segs[v].len() != 1 {
            continue;
        }
        let (cu, cv) = (lay.items[u].center(), lay.items[v].center());
        if cu == cv {
            continue;
        }
        if fits(lay, v, cu) {
            lay.items[v].pos = cu - (lay.items[v].ext_o - 1) / 2;
        } else if lay.in_segs[u].len() <= 1 && fits(lay, u, cv) {
            lay.items[u].pos = cv - (lay.items[u].ext_o - 1) / 2;
        }
    }
}

/// Inserts empty columns so each cluster box is wide enough for its title.
/// Shifting everything on one side of a line keeps all alignments intact.
fn widen_for_titles(lay: &mut Layout, d: &Diagram) {
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for it in &lay.items {
        if let ItemKind::Border { cluster, .. } = it.kind {
            if !clusters.iter().any(|c| c.0 == cluster) {
                clusters.push((cluster, it.path.len()));
            }
        }
    }
    clusters.sort_by_key(|c| Reverse(c.1));
    for (c, _) in clusters {
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for it in &lay.items {
            if let ItemKind::Border { cluster, left } = it.kind {
                if cluster == c {
                    if left {
                        lo = lo.min(it.pos);
                    } else {
                        hi = hi.max(it.pos);
                    }
                }
            }
        }
        let need = UnicodeWidthStr::width(d.clusters[c].title.as_str()) as i32 + 6;
        let have = hi - lo + 1;
        if have >= need {
            continue;
        }
        let delta = need - have;
        let dl = delta / 2;
        let dr = delta - dl;
        for it in lay.items.iter_mut() {
            if it.pos >= hi {
                it.pos += dr;
            } else if it.pos + it.ext_o - 1 <= lo {
                it.pos -= dl;
            }
        }
    }
}

fn normalize(lay: &mut Layout) {
    let min = lay.items.iter().map(|it| it.pos).min().unwrap_or(0);
    for it in lay.items.iter_mut() {
        it.pos -= min;
    }
}
