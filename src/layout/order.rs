//! Crossing minimisation: layer-by-layer barycenter sweeps plus adjacent
//! transpositions, keeping every cluster contiguous within each layer with
//! its two border items at the extremes.

use std::cmp::Ordering;

use super::{ItemKind, Layout, position};
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
    for (item, neighbours) in children.iter_mut().enumerate() {
        match discovery {
            Discovery::Declaration => {}
            Discovery::BackEdgesFirst | Discovery::BackEdgesLast => {
                neighbours.sort_by_key(|&child| {
                    let reversed = lay.out_segs[item].iter().any(|&segment| {
                        lay.segs[segment].to == child && lay.reversed[lay.segs[segment].edge]
                    });
                    matches!(discovery, Discovery::BackEdgesFirst) != reversed
                });
            }
            Discovery::Reverse => neighbours.reverse(),
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

impl RoutedScore {
    fn measure(diagram: &Diagram, layout: &Layout) -> Self {
        let routing = route::route(diagram, layout);
        Self {
            crossings: route::count_crossings(&routing),
            order_extent: layout
                .items
                .iter()
                .map(|item| item.pos + item.ext_o)
                .max()
                .unwrap_or(0),
            tracks: routing.tracks.iter().sum(),
        }
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
        if score.improves(&best_score) {
            best_score = score;
            best_items = layout.items.clone();
            best_layers = layout.layers.clone();
        }
    }
    layout.items = best_items;
    layout.layers = best_layers;
}
