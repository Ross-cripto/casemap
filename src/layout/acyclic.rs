//! Cycle removal that respects how the author wrote the diagram.
//!
//! Edges are added in declaration order; an edge that would close a cycle is
//! reversed instead. Authors usually declare the main flow first and feedback
//! edges ("retry", "callback") later, so this reverses the edges a reader
//! would also consider the "back" edges.

use std::collections::VecDeque;

pub fn reversed_edges(n: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut reversed = vec![false; edges.len()];
    for (i, &(u, v)) in edges.iter().enumerate() {
        if reaches(&adj, v, u) {
            reversed[i] = true;
            adj[v].push(u);
        } else {
            adj[u].push(v);
        }
    }
    reversed
}

fn reaches(adj: &[Vec<usize>], from: usize, to: usize) -> bool {
    if from == to {
        return true;
    }
    let mut seen = vec![false; adj.len()];
    let mut queue = VecDeque::from([from]);
    seen[from] = true;
    while let Some(u) = queue.pop_front() {
        for &v in &adj[u] {
            if v == to {
                return true;
            }
            if !seen[v] {
                seen[v] = true;
                queue.push_back(v);
            }
        }
    }
    false
}
