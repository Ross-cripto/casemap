//! Orthogonal edge routing between adjacent layers.
//!
//! Each node gets ports along its forward and backward sides. Outgoing edges
//! of the same style share one port and fan out as a bus; incoming edges each
//! get their own port. Ports are placed as close as possible to the edge they
//! serve (keeping their order and a one-cell gap), so an edge that can run
//! straight does. In the channel between two layers every bus with a
//! horizontal run is assigned a track (a row, or a column for LR). Tracks are
//! ordered so a vertical never cuts through a horizontal it could have
//! avoided, and non-overlapping runs share a track.

use crate::layout::{size::port_inset, ItemKind, Layout};
use crate::model::{Diagram, LineStyle, Mark, Shape};

#[derive(Clone, Debug)]
pub struct Dest {
    pub o: i32,
    pub arrow: bool,
}

#[derive(Clone, Debug)]
pub struct Group {
    /// Layer the group starts in; it runs through the channel after it.
    pub layer: usize,
    pub src: usize,
    pub src_o: i32,
    pub dests: Vec<Dest>,
    pub style: LineStyle,
    pub mark: Mark,
    pub arrow_src: bool,
    /// None when the group is a single straight line.
    pub track: Option<usize>,
}

impl Group {
    pub fn span(&self) -> (i32, i32) {
        self.dests
            .iter()
            .fold((self.src_o, self.src_o), |(a, b), d| {
                (a.min(d.o), b.max(d.o))
            })
    }
}

pub struct Routing {
    pub groups: Vec<Group>,
    /// Number of tracks used in the channel after each layer.
    pub tracks: Vec<usize>,
    /// Whether the channel after each layer has an arrowhead at its start.
    pub src_arrow: Vec<bool>,
}

pub fn route(d: &Diagram, lay: &Layout) -> Routing {
    let n = lay.items.len();
    let mut groups: Vec<Group> = Vec::new();
    let mut seg_group = vec![usize::MAX; lay.segs.len()];

    // Outgoing ports.
    for item in 0..n {
        let units = lay.out_units(d, item);
        if units.is_empty() {
            continue;
        }
        let mut prefs: Vec<(f64, usize)> = units
            .iter()
            .enumerate()
            .map(|(i, u)| {
                // A bus fanning out to several targets leaves from the centre;
                // a single edge leans toward its target so it can run straight.
                let pref = if u.segs.len() > 1 {
                    lay.items[item].center() as f64
                } else {
                    lay.items[lay.segs[u.segs[0]].to].center() as f64
                };
                (pref, i)
            })
            .collect();
        prefs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let slots = ports(lay, d, item, &prefs.iter().map(|p| p.0).collect::<Vec<_>>());
        let rank = lay.items[item].rank;
        for (&(_, ui), &o) in prefs.iter().zip(&slots) {
            let u = &units[ui];
            let g = groups.len();
            for &s in &u.segs {
                seg_group[s] = g;
            }
            groups.push(Group {
                layer: rank,
                src: item,
                src_o: o,
                dests: Vec::new(),
                style: u.style,
                mark: u.mark,
                arrow_src: u.arrow_src,
                track: None,
            });
        }
    }

    // Incoming ports, ordered by source position and pulled toward it.
    for item in 0..n {
        let mut ins = lay.in_segs[item].clone();
        if ins.is_empty() {
            continue;
        }
        ins.sort_by_key(|&s| (groups[seg_group[s]].src_o, s));
        let prefs: Vec<f64> = ins
            .iter()
            .map(|&s| groups[seg_group[s]].src_o as f64)
            .collect();
        let slots = ports(lay, d, item, &prefs);
        for (&s, &o) in ins.iter().zip(&slots) {
            let arrow = lay.arrow_at_dst(d, &lay.segs[s]);
            groups[seg_group[s]].dests.push(Dest { o, arrow });
        }
    }

    let nl = lay.layers.len();
    let mut tracks = vec![0usize; nl];
    let mut src_arrow = vec![false; nl];
    for l in 0..nl {
        let ids: Vec<usize> = (0..groups.len())
            .filter(|&g| groups[g].layer == l)
            .collect();
        src_arrow[l] = ids.iter().any(|&g| groups[g].arrow_src);
        tracks[l] = assign_tracks(&mut groups, &ids);
    }
    Routing {
        groups,
        tracks,
        src_arrow,
    }
}

/// Port positions for an item's side, one per preference (sorted ascending).
fn ports(lay: &Layout, d: &Diagram, item: usize, prefs: &[f64]) -> Vec<i32> {
    let it = &lay.items[item];
    let ItemKind::Node(n) = it.kind else {
        return vec![it.center(); prefs.len()];
    };
    let shape = d.nodes[n].shape;
    let k = prefs.len() as i32;
    let mut lo = it.pos + 1;
    let mut hi = it.pos + it.ext_o - 2;
    if lay.direction.is_vertical() {
        lo += port_inset(shape);
        hi -= port_inset(shape);
    } else if shape == Shape::Database {
        lo += 1; // skip the lid separator
    }
    // Keep ports off the cells beside the corners when there is room.
    if hi - lo - 2 >= 2 * (k - 1) {
        lo += 1;
        hi -= 1;
    }
    if k == 1 && lo > hi {
        return vec![it.center()];
    }
    // A preference the port can line up with is kept, so the edge runs straight.
    // Anything out of reach only says which side it leans to, which keeps
    // such ports gathered symmetrically around the centre.
    // Reachable preferences weigh much more than leanings.
    let c = it.center() as f64;
    let prefs: Vec<(f64, f64)> = prefs
        .iter()
        .map(|&x| {
            if x < lo as f64 {
                (c - 0.5, 1.0)
            } else if x > hi as f64 {
                (c + 0.5, 1.0)
            } else {
                (x, 10.0)
            }
        })
        .collect();
    assign_slots(&prefs, lo, hi)
}

/// Chooses increasing positions in [lo, hi], at least two apart, minimising
/// the weighted distance to the preferred positions `(position, weight)`.
fn assign_slots(prefs: &[(f64, f64)], lo: i32, hi: i32) -> Vec<i32> {
    let k = prefs.len();
    let s = (hi - lo + 1).max(0) as usize;
    if k == 0 {
        return Vec::new();
    }
    if s < 2 * (k - 1) + 1 {
        let start = (lo + hi) / 2 - (k as i32 - 1);
        return (0..k).map(|i| start + 2 * i as i32).collect();
    }
    let cost = |i: usize, j: usize| (prefs[i].0 - (lo + j as i32) as f64).abs() * prefs[i].1;
    let inf = f64::INFINITY;
    let mut dp = vec![vec![inf; s]; k];
    let mut from = vec![vec![0usize; s]; k];
    for j in 0..s {
        dp[0][j] = cost(0, j);
    }
    for i in 1..k {
        let (mut best, mut best_j) = (inf, 0);
        for j in 0..s {
            if j >= 2 && dp[i - 1][j - 2] < best {
                best = dp[i - 1][j - 2];
                best_j = j - 2;
            }
            if best < inf {
                dp[i][j] = best + cost(i, j);
                from[i][j] = best_j;
            }
        }
    }
    let mid = (s as f64 - 1.0) / 2.0;
    let mut j = (0..s)
        .min_by(|&a, &b| {
            dp[k - 1][a]
                .total_cmp(&dp[k - 1][b])
                .then((a as f64 - mid).abs().total_cmp(&(b as f64 - mid).abs()))
        })
        .unwrap_or(0);
    let mut out = vec![0; k];
    for i in (0..k).rev() {
        out[i] = lo + j as i32;
        if i > 0 {
            j = from[i][j];
        }
    }
    out
}

fn assign_tracks(groups: &mut [Group], ids: &[usize]) -> usize {
    let bent: Vec<usize> = ids
        .iter()
        .copied()
        .filter(|&g| {
            let (a, b) = groups[g].span();
            a != b
        })
        .collect();
    let m = bent.len();
    if m == 0 {
        return 0;
    }
    let spans: Vec<(i32, i32)> = bent.iter().map(|&g| groups[g].span()).collect();
    let inside = |x: i32, s: (i32, i32)| x >= s.0 && x <= s.1;

    // b must sit above a when b's source drop or a's target drops would
    // otherwise cut through the other's horizontal run.
    let mut succ = vec![Vec::new(); m];
    let mut preds = vec![Vec::new(); m];
    let mut indeg = vec![0usize; m];
    for a in 0..m {
        for b in 0..m {
            if a == b {
                continue;
            }
            let (ga, gb) = (&groups[bent[a]], &groups[bent[b]]);
            let b_above_a =
                inside(gb.src_o, spans[a]) || ga.dests.iter().any(|d| inside(d.o, spans[b]));
            if b_above_a {
                succ[b].push(a);
                preds[a].push(b);
                indeg[a] += 1;
            }
        }
    }

    // Topological order; on a cycle, take the least-constrained group.
    let mut done = vec![false; m];
    let mut order = Vec::with_capacity(m);
    while order.len() < m {
        let next = (0..m)
            .find(|&i| !done[i] && indeg[i] == 0)
            .or_else(|| (0..m).filter(|&i| !done[i]).min_by_key(|&i| (indeg[i], i)));
        let Some(i) = next else { break };
        done[i] = true;
        order.push(i);
        for &a in &succ[i] {
            indeg[a] = indeg[a].saturating_sub(1);
        }
    }

    let mut track = vec![usize::MAX; m];
    for &i in &order {
        let mut t = preds[i]
            .iter()
            .filter(|&&p| track[p] != usize::MAX)
            .map(|&p| track[p] + 1)
            .max()
            .unwrap_or(0);
        while (0..m)
            .any(|j| track[j] == t && spans[j].0 <= spans[i].1 + 1 && spans[i].0 <= spans[j].1 + 1)
        {
            t += 1;
        }
        track[i] = t;
    }
    for (k, &g) in bent.iter().enumerate() {
        groups[g].track = Some(track[k]);
    }
    track.iter().max().map_or(0, |t| t + 1)
}

#[cfg(test)]
mod tests {
    use super::assign_slots;

    #[test]
    fn slots_follow_preferences_with_spacing() {
        let w = |xs: &[f64]| xs.iter().map(|&x| (x, 1.0)).collect::<Vec<_>>();
        assert_eq!(assign_slots(&w(&[5.0]), 0, 10), vec![5]);
        assert_eq!(assign_slots(&w(&[4.5, 5.5]), 0, 10), vec![4, 6]);
        assert_eq!(assign_slots(&w(&[-3.0, 20.0]), 0, 10), vec![0, 10]);
        assert_eq!(assign_slots(&w(&[1.0, 2.0, 3.0]), 1, 5), vec![1, 3, 5]);
        // A reachable (heavy) preference wins over a leaning one.
        assert_eq!(assign_slots(&[(5.0, 10.0), (5.5, 1.0)], 0, 10), vec![5, 7]);
    }
}
