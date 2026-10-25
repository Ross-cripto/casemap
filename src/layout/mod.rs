//! Layered (Sugiyama-style) layout.
//!
//! Pipeline: cycle removal → ranking → long-edge splitting (dummy and label
//! items) → cluster border chains → crossing minimisation → sizing →
//! coordinate assignment. Coordinates are abstract: `rank` runs along the
//! flow direction and `pos`/`ext_o` along the order axis, so the same layout
//! serves every direction; the renderer maps it onto the screen.

mod acyclic;
mod order;
mod position;
mod rank;
pub mod size;

use crate::model::{Diagram, Direction, LineStyle};

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// A real diagram node.
    Node(usize),
    /// A point where a long edge passes through a layer.
    Dummy,
    /// A long-edge pass-through carrying the edge's label text.
    Label(String),
    /// One side of a cluster box in one layer.
    Border { cluster: usize, left: bool },
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    pub rank: usize,
    /// Cluster ancestry, outermost first.
    pub path: Vec<usize>,
    /// Extent along the order axis (always odd for items with a centre port).
    pub ext_o: i32,
    /// Extent along the rank axis.
    pub ext_r: i32,
    /// Start coordinate along the order axis.
    pub pos: i32,
}

impl Item {
    pub fn center(&self) -> i32 {
        self.pos + (self.ext_o - 1) / 2
    }

    pub fn is_border(&self) -> bool {
        matches!(self.kind, ItemKind::Border { .. })
    }

    pub fn is_node(&self) -> bool {
        matches!(self.kind, ItemKind::Node(_))
    }
}

/// Part of an edge between two adjacent layers.
#[derive(Clone, Copy, Debug)]
pub struct Seg {
    pub from: usize,
    pub to: usize,
    pub edge: usize,
    pub first: bool,
    pub last: bool,
}

/// A set of a node's outgoing segments that leave through one port.
pub struct Unit {
    pub arrow_src: bool,
    pub style: LineStyle,
    pub segs: Vec<usize>,
}

pub struct Layout {
    pub direction: Direction,
    pub items: Vec<Item>,
    pub layers: Vec<Vec<usize>>,
    pub segs: Vec<Seg>,
    pub out_segs: Vec<Vec<usize>>,
    pub in_segs: Vec<Vec<usize>>,
    /// Per diagram edge: drawn against the flow to break a cycle.
    pub reversed: Vec<bool>,
}

impl Layout {
    /// Whether this segment needs an arrowhead at its layout source.
    pub fn arrow_at_src(&self, d: &Diagram, s: &Seg) -> bool {
        if !s.first {
            return false;
        }
        let e = &d.edges[s.edge];
        if self.reversed[s.edge] {
            e.arrow_to
        } else {
            e.arrow_from
        }
    }

    /// Whether this segment needs an arrowhead at its layout target.
    pub fn arrow_at_dst(&self, d: &Diagram, s: &Seg) -> bool {
        if !s.last {
            return false;
        }
        let e = &d.edges[s.edge];
        if self.reversed[s.edge] {
            e.arrow_from
        } else {
            e.arrow_to
        }
    }

    /// Groups an item's outgoing segments by port. Plain forward edges of the
    /// same style share one port (drawn as a bus); edges that need an
    /// arrowhead at this end get a port of their own so the arrow is unambiguous.
    pub fn out_units(&self, d: &Diagram, item: usize) -> Vec<Unit> {
        let mut units: Vec<Unit> = Vec::new();
        for &s in &self.out_segs[item] {
            let seg = &self.segs[s];
            let style = d.edges[seg.edge].style;
            if self.arrow_at_src(d, seg) {
                units.push(Unit {
                    arrow_src: true,
                    style,
                    segs: vec![s],
                });
            } else if let Some(u) = units.iter_mut().find(|u| !u.arrow_src && u.style == style) {
                u.segs.push(s);
            } else {
                units.push(Unit {
                    arrow_src: false,
                    style,
                    segs: vec![s],
                });
            }
        }
        units
    }

    fn index_segments(&mut self) {
        let n = self.items.len();
        self.out_segs = vec![Vec::new(); n];
        self.in_segs = vec![Vec::new(); n];
        for (i, s) in self.segs.iter().enumerate() {
            self.out_segs[s.from].push(i);
            self.in_segs[s.to].push(i);
        }
    }
}

pub fn layout(d: &Diagram) -> Layout {
    let n = d.nodes.len();

    // 1. Break cycles.
    let pairs: Vec<(usize, usize)> = d.edges.iter().map(|e| (e.from, e.to)).collect();
    let reversed = acyclic::reversed_edges(n, &pairs);

    // 2. Rank. Labelled edges span two ranks so the label gets a row of its own.
    let ranked: Vec<(usize, usize, i32)> = d
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (a, b) = if reversed[i] {
                (e.to, e.from)
            } else {
                (e.from, e.to)
            };
            (a, b, if e.label.is_some() { 2 } else { 1 })
        })
        .collect();
    let ranks = rank::assign(n, &ranked);

    // 3. Items for real nodes.
    let mut items: Vec<Item> = Vec::new();
    let mut node_item = Vec::with_capacity(n);
    for (i, &r) in ranks.iter().enumerate() {
        node_item.push(items.len());
        items.push(Item {
            kind: ItemKind::Node(i),
            rank: r,
            path: d.cluster_path(d.node_cluster[i]),
            ext_o: 1,
            ext_r: 1,
            pos: 0,
        });
    }

    // 4. Split long edges into per-layer segments.
    let mut segs = Vec::new();
    for (e, &(a, b, _)) in ranked.iter().enumerate() {
        let (ia, ib) = (node_item[a], node_item[b]);
        let (ra, rb) = (items[ia].rank, items[ib].rank);
        let span = rb - ra;
        let path = common_prefix(&items[ia].path, &items[ib].path);
        let label_rank = d.edges[e]
            .label
            .as_ref()
            .filter(|_| span >= 2)
            .map(|_| ra + span / 2);
        let mut prev = ia;
        for step in 1..span {
            let r = ra + step;
            let kind = if Some(r) == label_rank {
                ItemKind::Label(d.edges[e].label.clone().unwrap_or_default())
            } else {
                ItemKind::Dummy
            };
            let id = items.len();
            items.push(Item {
                kind,
                rank: r,
                path: path.clone(),
                ext_o: 1,
                ext_r: 1,
                pos: 0,
            });
            segs.push(Seg {
                from: prev,
                to: id,
                edge: e,
                first: step == 1,
                last: false,
            });
            prev = id;
        }
        segs.push(Seg {
            from: prev,
            to: ib,
            edge: e,
            first: span == 1,
            last: true,
        });
    }

    // 5. Cluster border chains, one left and one right border per spanned rank.
    let nc = d.clusters.len();
    let mut range: Vec<Option<(usize, usize)>> = vec![None; nc];
    for it in &items {
        for &c in &it.path {
            range[c] = Some(match range[c] {
                None => (it.rank, it.rank),
                Some((lo, hi)) => (lo.min(it.rank), hi.max(it.rank)),
            });
        }
    }
    for (c, r) in range.iter().enumerate() {
        let Some((lo, hi)) = *r else { continue };
        let path = d.cluster_path(Some(c));
        for rank in lo..=hi {
            for left in [true, false] {
                items.push(Item {
                    kind: ItemKind::Border { cluster: c, left },
                    rank,
                    path: path.clone(),
                    ext_o: 1,
                    ext_r: 0,
                    pos: 0,
                });
            }
        }
    }

    let nl = items.iter().map(|it| it.rank + 1).max().unwrap_or(0);
    let mut layers = vec![Vec::new(); nl];
    for (i, it) in items.iter().enumerate() {
        layers[it.rank].push(i);
    }

    let mut lay = Layout {
        direction: d.direction,
        items,
        layers,
        segs,
        out_segs: Vec::new(),
        in_segs: Vec::new(),
        reversed,
    };
    lay.index_segments();
    order::minimize(&mut lay);
    size::assign(&mut lay, d);
    position::assign(&mut lay, d);
    lay
}

fn common_prefix(a: &[usize], b: &[usize]) -> Vec<usize> {
    a.iter()
        .zip(b)
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| *x)
        .collect()
}
