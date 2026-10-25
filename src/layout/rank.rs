//! Rank assignment on an acyclic graph with per-edge minimum lengths.
//!
//! Starts from longest-path ranking, then repeatedly moves each node to the
//! median of its preferred ranks within its feasible window. That is
//! coordinate descent on total edge length, which pulls sources down next to
//! their children and sinks up next to their parents instead of leaving them
//! stranded at the top or bottom.

pub fn assign(n: usize, edges: &[(usize, usize, i32)]) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let mut ins: Vec<Vec<(usize, i32)>> = vec![Vec::new(); n];
    let mut outs: Vec<Vec<(usize, i32)>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for &(u, v, len) in edges {
        outs[u].push((v, len));
        ins[v].push((u, len));
        indeg[v] += 1;
    }

    // Kahn topological order, stable by index.
    let mut topo = Vec::with_capacity(n);
    let mut ready: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    ready.reverse();
    while let Some(u) = ready.pop() {
        topo.push(u);
        let mut next = Vec::new();
        for &(v, _) in &outs[u] {
            indeg[v] -= 1;
            if indeg[v] == 0 {
                next.push(v);
            }
        }
        next.sort_unstable_by(|a, b| b.cmp(a));
        ready.extend(next);
    }

    let mut rank = vec![0i32; n];
    for &v in &topo {
        rank[v] = ins[v]
            .iter()
            .map(|&(u, len)| rank[u] + len)
            .max()
            .unwrap_or(0);
    }

    for round in 0..16 {
        let mut changed = false;
        let iter: Box<dyn Iterator<Item = &usize>> = if round % 2 == 0 {
            Box::new(topo.iter().rev())
        } else {
            Box::new(topo.iter())
        };
        for &v in iter {
            if ins[v].is_empty() && outs[v].is_empty() {
                continue;
            }
            let lo = ins[v]
                .iter()
                .map(|&(u, len)| rank[u] + len)
                .max()
                .unwrap_or(i32::MIN);
            let hi = outs[v]
                .iter()
                .map(|&(w, len)| rank[w] - len)
                .min()
                .unwrap_or(i32::MAX);
            let mut prefs: Vec<i32> = ins[v]
                .iter()
                .map(|&(u, len)| rank[u] + len)
                .chain(outs[v].iter().map(|&(w, len)| rank[w] - len))
                .collect();
            prefs.sort_unstable();
            let m = prefs.len();
            let target = if m % 2 == 1 {
                prefs[m / 2]
            } else {
                rank[v].clamp(prefs[m / 2 - 1], prefs[m / 2])
            };
            let target = target.clamp(lo, hi.max(lo));
            if target != rank[v] {
                rank[v] = target;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let min = *rank.iter().min().unwrap_or(&0);
    rank.iter().map(|&r| (r - min) as usize).collect()
}
