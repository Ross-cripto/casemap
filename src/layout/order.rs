//! Crossing minimisation: layer-by-layer barycenter sweeps plus adjacent
//! transpositions, keeping every cluster contiguous within each layer with
//! its two border items at the extremes.

use std::cmp::Ordering;

use super::{Item, ItemKind, Layout, position};
use crate::{model::Diagram, route};

pub fn minimize(lay: &mut Layout) {
    minimize_with_discovery(lay, Discovery::Declaration);
}

#[derive(Clone, Copy)]
enum Discovery {
    Declaration,
    BackEdgesFirst,
    BackEdgesLast,
    Reverse,
    Branches(usize),
}

fn minimize_with_discovery(lay: &mut Layout, discovery: Discovery) {
    let n = lay.items.len();
    let nl = lay.layers.len();
    if nl == 0 {
        return;
    }
    let mut up = vec![Vec::new(); n];
    let mut down = vec![Vec::new(); n];
    for s in &lay.segs {
        down[s.from].push(s.to);
        up[s.to].push(s.from);
    }

    initial_order(lay, &discovery_children(lay, &down, discovery));
    let mut best = lay.layers.clone();
    let mut best_c = total_crossings(lay);

    for iter in 0..32 {
        if best_c == 0 {
            break;
        }
        if iter % 2 == 0 {
            for l in 1..nl {
                sweep_layer(lay, l, &up);
            }
        } else {
            for l in (0..nl - 1).rev() {
                sweep_layer(lay, l, &down);
            }
        }
        transpose(lay, &up, &down);
        let c = total_crossings(lay);
        if c < best_c {
            best_c = c;
            best = lay.layers.clone();
        }
    }
    lay.layers = best;
}

/// Keeps feedback chains together at a layer's outside when declaration order
/// threads them through forward branches.
fn discovery_children(lay: &Layout, down: &[Vec<usize>], discovery: Discovery) -> Vec<Vec<usize>> {
    let mut children = down.to_vec();
    for (turn, item) in lay.items.iter().enumerate() {
        if !matches!(item.kind, ItemKind::LoopTurn) {
            continue;
        }
        let target = lay.out_segs[turn]
            .iter()
            .map(|&segment| &lay.segs[segment])
            .find(|segment| segment.last && !segment.first)
            .map(|segment| segment.to)
            .expect("loop turn has a target branch");
        children[turn].retain(|&child| child != target);
        children[target].push(turn);
    }
    let mut branch = 0;
    for (item, neighbours) in children.iter_mut().enumerate() {
        match discovery {
            Discovery::Declaration => {}
            Discovery::BackEdgesFirst | Discovery::BackEdgesLast => {
                neighbours.sort_by_key(|&child| {
                    let reversed = matches!(lay.items[child].kind, ItemKind::LoopTurn)
                        || lay.out_segs[item].iter().any(|&segment| {
                            lay.segs[segment].to == child && lay.reversed[lay.segs[segment].edge]
                        });
                    matches!(discovery, Discovery::BackEdgesFirst) != reversed
                });
            }
            Discovery::Reverse => neighbours.reverse(),
            Discovery::Branches(choices) => {
                if neighbours.len() > 1 && lay.items[item].is_node() {
                    if choices.checked_shr(branch).unwrap_or(0) & 1 != 0 {
                        neighbours.reverse();
                    }
                    branch += 1;
                }
            }
        }
    }
    children
}

fn index_map(lay: &Layout) -> Vec<usize> {
    let mut idx = vec![0; lay.items.len()];
    for layer in &lay.layers {
        for (i, &it) in layer.iter().enumerate() {
            idx[it] = i;
        }
    }
    idx
}

/// Depth-first discovery order from the sources, which keeps chains together.
fn initial_order(lay: &mut Layout, down: &[Vec<usize>]) {
    let n = lay.items.len();
    let mut has_in = vec![false; n];
    for s in &lay.segs {
        has_in[s.to] = true;
    }
    let roots: Vec<usize> = (0..n)
        .filter(|&i| lay.items[i].is_node() && !has_in[i])
        .chain((0..n).filter(|&i| lay.items[i].is_node() && has_in[i]))
        .collect();

    let mut seen = vec![false; n];
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); lay.layers.len()];
    for root in roots {
        let mut stack = vec![root];
        while let Some(it) = stack.pop() {
            if seen[it] {
                continue;
            }
            seen[it] = true;
            layers[lay.items[it].rank].push(it);
            for &child in down[it].iter().rev() {
                if !seen[child] {
                    stack.push(child);
                }
            }
        }
    }
    for i in 0..n {
        if !seen[i] {
            layers[lay.items[i].rank].push(i);
        }
    }
    lay.layers = layers;

    let idx = index_map(lay);
    let key: Vec<f64> = idx.iter().map(|&i| i as f64).collect();
    for l in 0..lay.layers.len() {
        let layer = lay.layers[l].clone();
        lay.layers[l] = sort_level(lay, &layer, 0, &key);
    }
}

fn sweep_layer(lay: &mut Layout, l: usize, nbrs: &[Vec<usize>]) {
    let idx = index_map(lay);
    let mut key = vec![0.0f64; lay.items.len()];
    for &it in &lay.layers[l] {
        key[it] = if nbrs[it].is_empty() {
            idx[it] as f64
        } else {
            nbrs[it].iter().map(|&w| idx[w] as f64).sum::<f64>() / nbrs[it].len() as f64
        };
    }
    let layer = lay.layers[l].clone();
    lay.layers[l] = sort_level(lay, &layer, 0, &key);
}

/// Sorts a layer by key while keeping clusters contiguous: each cluster is a
/// block sorted by its members' mean key, recursively, and its borders are
/// pinned to the block's ends.
fn sort_level(lay: &Layout, list: &[usize], depth: usize, key: &[f64]) -> Vec<usize> {
    let mut left = None;
    let mut right = None;
    let mut units: Vec<(f64, Vec<usize>)> = Vec::new();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for &it in list {
        let item = &lay.items[it];
        if let ItemKind::Border { left: is_left, .. } = item.kind {
            if item.path.len() == depth {
                if is_left {
                    left = Some(it);
                } else {
                    right = Some(it);
                }
                continue;
            }
        }
        if item.path.len() <= depth {
            units.push((key[it], vec![it]));
        } else {
            let c = item.path[depth];
            match groups.iter().find(|(gc, _)| *gc == c) {
                Some(&(_, u)) => units[u].1.push(it),
                None => {
                    groups.push((c, units.len()));
                    units.push((0.0, vec![it]));
                }
            }
        }
    }
    for &(_, u) in &groups {
        let members = std::mem::take(&mut units[u].1);
        let vals: Vec<f64> = members
            .iter()
            .filter(|&&m| !lay.items[m].is_border())
            .map(|&m| key[m])
            .collect();
        let k = if vals.is_empty() {
            members.iter().map(|&m| key[m]).sum::<f64>() / members.len() as f64
        } else {
            vals.iter().sum::<f64>() / vals.len() as f64
        };
        units[u] = (k, sort_level(lay, &members, depth + 1, key));
    }
    units.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
    let mut out = Vec::with_capacity(list.len());
    out.extend(left);
    for (_, u) in units {
        out.extend(u);
    }
    out.extend(right);
    out
}

fn pair_crossings(u: usize, v: usize, nbrs: &[Vec<usize>], idx: &[usize]) -> usize {
    let mut c = 0;
    for &a in &nbrs[u] {
        for &b in &nbrs[v] {
            if idx[a] > idx[b] {
                c += 1;
            }
        }
    }
    c
}

/// Swaps adjacent siblings whenever that reduces crossings with both neighbours.
fn transpose(lay: &mut Layout, up: &[Vec<usize>], down: &[Vec<usize>]) {
    let mut idx = index_map(lay);
    for _ in 0..8 {
        let mut improved = false;
        for l in 0..lay.layers.len() {
            for i in 0..lay.layers[l].len().saturating_sub(1) {
                let (u, v) = (lay.layers[l][i], lay.layers[l][i + 1]);
                let (iu, iv) = (&lay.items[u], &lay.items[v]);
                if iu.is_border() || iv.is_border() || iu.path != iv.path {
                    continue;
                }
                let keep = pair_crossings(u, v, up, &idx) + pair_crossings(u, v, down, &idx);
                let swap = pair_crossings(v, u, up, &idx) + pair_crossings(v, u, down, &idx);
                if swap < keep {
                    lay.layers[l].swap(i, i + 1);
                    idx[u] = i + 1;
                    idx[v] = i;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

pub fn total_crossings(lay: &Layout) -> usize {
    let idx = index_map(lay);
    let mut by_layer: Vec<Vec<(usize, usize)>> = vec![Vec::new(); lay.layers.len()];
    for s in &lay.segs {
        by_layer[lay.items[s.from].rank].push((idx[s.from], idx[s.to]));
    }
    let mut total = 0;
    for pairs in &by_layer {
        for i in 0..pairs.len() {
            for j in i + 1..pairs.len() {
                let (a, b) = (pairs[i], pairs[j]);
                if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                    total += 1;
                }
            }
        }
    }
    total
}

struct RoutedScore {
    crossings: usize,
    order_extent: i32,
    tracks: usize,
}

struct LoopChain {
    items: Vec<usize>,
    source_rank: usize,
}

struct RoutedCandidate {
    score: RoutedScore,
    items: Vec<Item>,
    layers: Vec<Vec<usize>>,
}

impl RoutedCandidate {
    fn capture(diagram: &Diagram, layout: &Layout) -> Self {
        Self {
            score: RoutedScore::measure(diagram, layout),
            items: layout.items.clone(),
            layers: layout.layers.clone(),
        }
    }

    fn consider(&mut self, diagram: &Diagram, layout: &Layout) {
        let score = RoutedScore::measure(diagram, layout);
        if score.improves_loop_order(&self.score) {
            self.score = score;
            self.items = layout.items.clone();
            self.layers = layout.layers.clone();
        }
    }

    fn restore(self, layout: &mut Layout) {
        layout.items = self.items;
        layout.layers = self.layers;
    }
}

/// Keeps the entire return path outside forward branches, comparing both sides
/// with routed crossings before using width and track count to break ties.
pub(super) fn place_loops_outside(diagram: &Diagram, layout: &mut Layout) {
    let chains = loop_chains(layout);
    if chains.is_empty() {
        return;
    }
    let mut best = RoutedCandidate::capture(diagram, layout);
    compare_branch_orders(diagram, layout, &mut best);
    compare_loop_sides(diagram, layout, &chains, &mut best);
    layout.items = best.items.clone();
    layout.layers = best.layers.clone();
    mirror_order(layout);
    position::assign(layout, diagram);
    best.consider(diagram, layout);
    best.restore(layout);
}

/// A forward branch may have to move inward to expose a face for its return.
fn compare_branch_orders(diagram: &Diagram, layout: &mut Layout, best: &mut RoutedCandidate) {
    const MAXIMUM_BRANCH_CHOICES: usize = 8;
    let original_items = layout.items.clone();
    let down: Vec<Vec<usize>> = layout
        .out_segs
        .iter()
        .map(|segments| {
            segments
                .iter()
                .map(|&segment| layout.segs[segment].to)
                .collect()
        })
        .collect();
    let children = discovery_children(layout, &down, Discovery::Declaration);
    let branches = children
        .iter()
        .enumerate()
        .filter(|(item, neighbours)| layout.items[*item].is_node() && neighbours.len() > 1)
        .count();
    for choices in 0..1usize << branches.min(MAXIMUM_BRANCH_CHOICES) {
        layout.items = original_items.clone();
        minimize_with_discovery(layout, Discovery::Branches(choices));
        position::assign(layout, diagram);
        best.consider(diagram, layout);
    }
}

/// Side choices move whole return chains so one trial cannot thread a loop
/// between forward branches in only part of its span.
fn compare_loop_sides(
    diagram: &Diagram,
    layout: &mut Layout,
    chains: &[LoopChain],
    best: &mut RoutedCandidate,
) {
    const MAXIMUM_EXHAUSTIVE_LOOPS: usize = 6;
    let original_items = best.items.clone();
    let original_layers = best.layers.clone();
    let candidates = 1usize << chains.len().min(MAXIMUM_EXHAUSTIVE_LOOPS);
    for sides in 0..candidates {
        layout.items = original_items.clone();
        layout.layers = original_layers.clone();
        order_loop_sides(layout, chains, sides);
        position::assign(layout, diagram);
        best.consider(diagram, layout);
    }
}

/// Reflection exposes the other outside face without changing cluster borders.
fn mirror_order(layout: &mut Layout) {
    let keys: Vec<f64> = index_map(layout)
        .iter()
        .map(|&index| -(index as f64))
        .collect();
    for rank in 0..layout.layers.len() {
        layout.layers[rank] = sort_level(layout, &layout.layers[rank], 0, &keys);
    }
}

/// The turn and pass-throughs must choose one side together to avoid zig-zags.
fn loop_chains(layout: &Layout) -> Vec<LoopChain> {
    let mut chains = Vec::new();
    for (turn, item) in layout.items.iter().enumerate() {
        if !matches!(item.kind, ItemKind::LoopTurn) {
            continue;
        }
        let edge = layout.segs[layout.out_segs[turn][0]].edge;
        let mut items = vec![turn];
        let mut source_rank = item.rank;
        for segment in &layout.segs {
            if segment.edge != edge {
                continue;
            }
            if layout.items[segment.to].is_node() {
                source_rank = source_rank.max(layout.items[segment.to].rank);
            } else {
                items.push(segment.to);
            }
        }
        chains.push(LoopChain { items, source_rank });
    }
    chains
}

/// Longer returns sit outside shorter ones so their top turns can nest.
fn order_loop_sides(layout: &mut Layout, chains: &[LoopChain], sides: usize) {
    let indices = index_map(layout);
    let mut keys: Vec<f64> = indices.iter().map(|&index| index as f64).collect();
    let outside = layout.items.len() as f64;
    for (index, chain) in chains.iter().enumerate() {
        let distance = outside + chain.source_rank as f64 + index as f64 / chains.len() as f64;
        let right = sides.checked_shr(index as u32).unwrap_or(0) & 1 != 0;
        let key = if right { distance } else { -distance };
        for &item in &chain.items {
            keys[item] = key;
        }
    }
    for rank in 0..layout.layers.len() {
        layout.layers[rank] = sort_level(layout, &layout.layers[rank], 0, &keys);
    }
}

impl RoutedScore {
    fn measure(diagram: &Diagram, layout: &Layout) -> Self {
        let routing = route::route(diagram, layout);
        Self {
            crossings: route::count_crossings(&routing, layout),
            order_extent: layout
                .items
                .iter()
                .map(|item| item.pos + item.ext_o)
                .max()
                .unwrap_or(0),
            tracks: routing.tracks.iter().sum(),
        }
    }

    fn improves_loop_order(&self, best: &Self) -> bool {
        (self.crossings, self.order_extent, self.tracks)
            < (best.crossings, best.order_extent, best.tracks)
    }

    fn improves(&self, best: &Self) -> bool {
        self.crossings < best.crossings
            && self.order_extent <= best.order_extent
            && self.tracks <= best.tracks
    }
}

/// Compares feedback-chain discovery orders using routed geometry because
/// equal layer crossing counts can hide different bus and port crossings.
pub(super) fn refine_routed(diagram: &Diagram, layout: &mut Layout) {
    let mut best_score = RoutedScore::measure(diagram, layout);
    if best_score.crossings == 0 {
        return;
    }
    let original_items = layout.items.clone();
    let mut best_items = original_items.clone();
    let mut best_layers = layout.layers.clone();
    for discovery in [
        Discovery::BackEdgesFirst,
        Discovery::BackEdgesLast,
        Discovery::Reverse,
    ] {
        if best_score.crossings == 0 {
            break;
        }
        layout.items = original_items.clone();
        minimize_with_discovery(layout, discovery);
        position::assign(layout, diagram);
        let score = RoutedScore::measure(diagram, layout);
        let improves = if layout.reversed.iter().any(|&reversed| reversed) {
            score.improves_loop_order(&best_score)
        } else {
            score.improves(&best_score)
        };
        if improves {
            best_score = score;
            best_items = layout.items.clone();
            best_layers = layout.layers.clone();
        }
    }
    layout.items = best_items;
    layout.layers = best_layers;
}
