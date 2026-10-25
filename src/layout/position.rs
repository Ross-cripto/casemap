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
//! 5. Insert columns where a cluster is too narrow for its title.

use std::cmp::Reverse;

use unicode_width::UnicodeWidthStr;

use super::{Item, ItemKind, Layout};
use crate::model::Diagram;

struct Spacing {
    node: i32,
    thin: i32,
    border: i32,
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
    let sp = if lay.direction.is_vertical() {
        Spacing {
            node: 4,
            thin: 2,
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
    if lay.direction.is_vertical() {
        widen_for_titles(lay, d);
    }
    normalize(lay);
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
                ItemKind::Dummy | ItemKind::Label(_) => 100.0,
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
