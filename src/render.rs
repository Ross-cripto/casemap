//! Turns a routed layout into text.
//!
//! The rank axis is laid out as rows: each layer, then a channel holding
//! cluster bottom borders, the routing tracks, cluster top borders and the
//! arrowhead row. Everything is drawn in abstract (rank, order) coordinates
//! and mapped onto the screen per direction, so TD/BT/LR/RL share one path.

use unicode_width::UnicodeWidthStr;

use crate::canvas::{Canvas, Class, Owner, Pen};
use crate::layout::{ItemKind, Layout};
use crate::model::{Diagram, Direction, LineStyle, Mark, Node, Shape};
use crate::route::{Dest, Routing};

pub fn bold_line(text: &str, color: bool) -> String {
    if color {
        format!("\x1b[1m{text}\x1b[0m\n")
    } else {
        format!("{text}\n")
    }
}

pub struct Options {
    pub ascii: bool,
    pub color: bool,
}

struct Rows {
    layer_start: Vec<i32>,
    track_start: Vec<i32>,
    track_off: Vec<i32>,
    open_row: Vec<i32>,
    close_row: Vec<i32>,
    total: i32,
}

struct Clusters {
    rank: Vec<Option<(usize, usize)>>,
    order: Vec<Option<(i32, i32)>>,
    depth: Vec<usize>,
}

pub fn render(d: &Diagram, lay: &Layout, rt: &Routing, opts: &Options) -> String {
    let nl = lay.layers.len();
    if nl == 0 {
        return String::new();
    }
    let dir = lay.direction;
    let vertical = dir.is_vertical();
    let layer_ext: Vec<i32> = lay
        .layers
        .iter()
        .map(|l| {
            l.iter()
                .map(|&i| lay.items[i].ext_r)
                .max()
                .unwrap_or(1)
                .max(1)
        })
        .collect();
    let cl = collect_clusters(d, lay);

    // Rows; for LR the cluster title runs along the rank axis, so pad channels until it fits.
    let min_region = if vertical { 1 } else { 3 };
    let mut extra_pad = vec![0i32; nl];
    let rows = loop {
        let rows = compute_rows(nl, &layer_ext, &cl, rt, min_region, &extra_pad);
        if vertical {
            break rows;
        }
        let mut changed = false;
        for c in 0..d.clusters.len() {
            if let Some((_, hi)) = cl.rank[c] {
                let have = rows.close_row[c] - rows.open_row[c] + 1;
                let need = UnicodeWidthStr::width(d.clusters[c].title.as_str()) as i32 + 6;
                if have < need {
                    extra_pad[hi] += need - have;
                    changed = true;
                }
            }
        }
        if !changed {
            break rows;
        }
    };

    let o_total = lay
        .items
        .iter()
        .map(|it| it.pos + it.ext_o)
        .max()
        .unwrap_or(1);
    let r_total = rows.total;
    let (height, width) = if vertical {
        (r_total, o_total)
    } else {
        (o_total, r_total)
    };
    let mut cv = Canvas::new(height, width);
    let map = |r: i32, o: i32| -> (i32, i32) {
        match dir {
            Direction::TopDown => (r, o),
            Direction::BottomUp => (r_total - 1 - r, o),
            Direction::LeftRight => (o, r),
            Direction::RightLeft => (o, r_total - 1 - r),
        }
    };
    let rect = |r0: i32, r1: i32, o0: i32, o1: i32| -> (i32, i32, i32, i32) {
        let (a, b) = map(r0, o0);
        let (c, e) = map(r1, o1);
        (a.min(c), b.min(e), a.max(c), b.max(e))
    };
    let (fwd_arrow, back_arrow) = match dir {
        Direction::TopDown => ('▼', '▲'),
        Direction::BottomUp => ('▲', '▼'),
        Direction::LeftRight => ('▶', '◀'),
        Direction::RightLeft => ('◀', '▶'),
    };

    cv.set_ink(Mark::Same);
    // Cluster boxes.
    let mut titles = Vec::new();
    for c in 0..d.clusters.len() {
        let (Some(_), Some((o0, o1))) = (cl.rank[c], cl.order[c]) else {
            continue;
        };
        let (top, left, bottom, right) = rect(rows.open_row[c], rows.close_row[c], o0, o1);
        draw_box(
            &mut cv,
            top,
            left,
            bottom,
            right,
            Owner::Cluster(c as u32),
            Pen::Faint,
        );
        titles.push((top, left, right, d.clusters[c].title.clone()));
    }

    // Nodes.
    for (i, it) in lay.items.iter().enumerate() {
        let ItemKind::Node(n) = it.kind else { continue };
        let r0 = rows.layer_start[it.rank];
        let (top, left, bottom, right) = rect(r0, r0 + it.ext_r - 1, it.pos, it.pos + it.ext_o - 1);
        cv.set_ink(d.nodes[n].mark);
        draw_node(
            &mut cv,
            top,
            left,
            bottom,
            right,
            &d.nodes[n],
            Owner::Node(i as u32),
        );
    }

    // Edges.
    for (gi, g) in rt.groups.iter().enumerate() {
        cv.set_ink(g.mark);
        let l = g.layer;
        let src = &lay.items[g.src];
        let src_r = match src.kind {
            ItemKind::Dummy => rows.layer_start[l],
            _ => rows.layer_start[l] + src.ext_r - 1,
        };
        let next_start = rows.layer_start[l + 1];
        let dest_r = |dst: &Dest| {
            if dst.arrow {
                next_start - 1
            } else {
                next_start
            }
        };
        let pen = match g.style {
            LineStyle::Solid => Pen::Round,
            LineStyle::Dotted => Pen::Dashed,
            LineStyle::Thick => Pen::Heavy,
        };
        let owner = Owner::Edge(gi as u32);
        let mut seg = |r0: i32, o0: i32, r1: i32, o1: i32| {
            let (a, b) = map(r0, o0);
            let (c, e) = map(r1, o1);
            cv.line(a, b, c, e, owner, pen);
        };
        match g.track {
            None => {
                for dst in &g.dests {
                    seg(src_r, g.src_o, dest_r(dst), dst.o);
                }
            }
            Some(t) => {
                let tr = rows.track_start[l] + rows.track_off[l] + t as i32;
                let (lo, hi) = g.span();
                if !matches!(src.kind, ItemKind::LoopTurn) {
                    seg(src_r, g.src_o, tr, g.src_o);
                }
                seg(tr, lo, tr, hi);
                for dst in &g.dests {
                    seg(tr, dst.o, dest_r(dst), dst.o);
                }
            }
        }
        for dst in g.dests.iter().filter(|d| d.arrow) {
            let (r, c) = map(next_start - 1, dst.o);
            cv.put(r, c, fwd_arrow, Class::Arrow);
        }
        if g.arrow_src {
            let (r, c) = map(src_r + 1, g.src_o);
            cv.put(r, c, back_arrow, Class::Arrow);
        }
    }

    // Edge labels.
    for (i, it) in lay.items.iter().enumerate() {
        let ItemKind::Label(text) = &it.kind else {
            continue;
        };
        cv.set_ink(
            lay.in_segs[i]
                .first()
                .map(|&s| d.edges[lay.segs[s].edge].mark)
                .unwrap_or_default(),
        );
        let r0 = rows.layer_start[it.rank];
        let (top, left, _, right) = rect(r0, r0 + it.ext_r - 1, it.pos, it.pos + it.ext_o - 1);
        center_text(
            &mut cv,
            top,
            left,
            right,
            &format!(" {text} "),
            Class::EdgeLabel,
        );
    }

    cv.set_ink(Mark::Same);
    // Cluster titles go last so they sit on top of their border but never over an edge.
    for (top, left, right, title) in titles {
        let avail = (right - left - 3).max(0) as usize;
        let label = truncate(&format!(" {title} "), avail);
        let mut col = left + 2;
        for ch in label.chars() {
            cv.put_soft(top, col, ch, Class::ClusterTitle);
            col += UnicodeWidthStr::width(ch.to_string().as_str()) as i32;
        }
    }

    cv.render(opts.ascii, opts.color)
}

fn collect_clusters(d: &Diagram, lay: &Layout) -> Clusters {
    let nc = d.clusters.len();
    let mut rank: Vec<Option<(usize, usize)>> = vec![None; nc];
    let mut lo = vec![i32::MAX; nc];
    let mut hi = vec![i32::MIN; nc];
    for it in &lay.items {
        if let ItemKind::Border { cluster, left } = it.kind {
            rank[cluster] = Some(match rank[cluster] {
                None => (it.rank, it.rank),
                Some((a, b)) => (a.min(it.rank), b.max(it.rank)),
            });
            if left {
                lo[cluster] = lo[cluster].min(it.pos);
            } else {
                hi[cluster] = hi[cluster].max(it.pos);
            }
        }
    }
    let order = (0..nc).map(|c| rank[c].map(|_| (lo[c], hi[c]))).collect();
    let depth = (0..nc).map(|c| d.cluster_path(Some(c)).len()).collect();
    Clusters { rank, order, depth }
}

fn compute_rows(
    nl: usize,
    layer_ext: &[i32],
    cl: &Clusters,
    rt: &Routing,
    min_region: i32,
    extra_pad: &[i32],
) -> Rows {
    let nc = cl.rank.len();
    let mut open_row = vec![0; nc];
    let mut close_row = vec![0; nc];
    let depths = |pick: &dyn Fn((usize, usize)) -> bool| -> Vec<usize> {
        let mut v: Vec<usize> = (0..nc)
            .filter(|&c| cl.rank[c].is_some_and(pick))
            .map(|c| cl.depth[c])
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    };

    // Cluster top borders before a layer, outermost first.
    let open = |rank: usize, r: &mut i32, open_row: &mut Vec<i32>| {
        let ds = depths(&|(lo, _)| lo == rank);
        for (k, &dep) in ds.iter().enumerate() {
            for (cluster, row) in open_row.iter_mut().enumerate() {
                if cl.depth[cluster] == dep && cl.rank[cluster].is_some_and(|(lo, _)| lo == rank) {
                    *row = *r + k as i32;
                }
            }
        }
        *r += ds.len() as i32;
        !ds.is_empty()
    };

    let mut layer_start = vec![0; nl];
    let mut track_start = vec![0; nl];
    let mut track_off = vec![0; nl];
    let mut r = 0;
    if open(0, &mut r, &mut open_row) {
        r += 1;
    }
    for l in 0..nl {
        layer_start[l] = r;
        r += layer_ext[l];

        // Cluster bottom borders after the layer, innermost first.
        let mut ds = depths(&|(_, hi)| hi == l);
        ds.reverse();
        if !ds.is_empty() {
            r += 1 + extra_pad[l];
            for (k, &dep) in ds.iter().enumerate() {
                for (cluster, row) in close_row.iter_mut().enumerate() {
                    if cl.depth[cluster] == dep && cl.rank[cluster].is_some_and(|(_, hi)| hi == l) {
                        *row = r + k as i32;
                    }
                }
            }
            r += ds.len() as i32;
        }
        if l + 1 == nl {
            break;
        }
        if rt.src_arrow[l] && ds.is_empty() {
            r += 1;
        }
        let nt = rt.tracks[l] as i32;
        let region = nt.max(min_region);
        track_start[l] = r;
        track_off[l] = (region - nt) / 2;
        r += region;
        open(l + 1, &mut r, &mut open_row);
        r += 1; // arrowhead row
    }
    Rows {
        layer_start,
        track_start,
        track_off,
        open_row,
        close_row,
        total: r,
    }
}

fn draw_box(cv: &mut Canvas, top: i32, left: i32, bottom: i32, right: i32, owner: Owner, pen: Pen) {
    cv.line(top, left, top, right, owner, pen);
    cv.line(bottom, left, bottom, right, owner, pen);
    cv.line(top, left, bottom, left, owner, pen);
    cv.line(top, right, bottom, right, owner, pen);
}

fn draw_node(
    cv: &mut Canvas,
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
    node: &Node,
    owner: Owner,
) {
    let mut text_top = top + 1;
    match node.shape {
        Shape::Rect => draw_box(cv, top, left, bottom, right, owner, Pen::Square),
        Shape::Round | Shape::Stadium | Shape::Circle => {
            draw_box(cv, top, left, bottom, right, owner, Pen::Round)
        }
        Shape::Subroutine => {
            draw_box(cv, top, left, bottom, right, owner, Pen::Square);
            cv.line(top, left + 1, bottom, left + 1, owner, Pen::Square);
            cv.line(top, right - 1, bottom, right - 1, owner, Pen::Square);
        }
        Shape::Database => {
            draw_box(cv, top, left, bottom, right, owner, Pen::Round);
            cv.line(top + 1, left, top + 1, right, owner, Pen::Round);
            text_top = top + 2;
        }
        Shape::Diamond | Shape::Hexagon => {
            // Lines run under the slanted corners so their ends resolve to '─'.
            cv.line(top, left + 1, top, right - 1, owner, Pen::Square);
            cv.line(bottom, left + 1, bottom, right - 1, owner, Pen::Square);
            cv.put(top, left + 1, '╱', Class::NodeBorder);
            cv.put(top, right - 1, '╲', Class::NodeBorder);
            cv.put(bottom, left + 1, '╲', Class::NodeBorder);
            cv.put(bottom, right - 1, '╱', Class::NodeBorder);
            let mid = (top + bottom) / 2;
            for r in top + 1..bottom {
                let (l, rr) = if r == mid { ('<', '>') } else { ('│', '│') };
                cv.put(r, left, l, Class::NodeBorder);
                cv.put(r, right, rr, Class::NodeBorder);
            }
        }
    }
    let avail = bottom - text_top;
    let n = node.label.len() as i32;
    let first = text_top + ((avail - n) / 2).max(0);
    for (k, line) in node.label.iter().enumerate() {
        center_text(
            cv,
            first + k as i32,
            left + 1,
            right - 1,
            line,
            Class::NodeText,
        );
    }
}

fn center_text(cv: &mut Canvas, row: i32, left: i32, right: i32, s: &str, class: Class) {
    let w = UnicodeWidthStr::width(s) as i32;
    let start = left + ((right - left + 1 - w) / 2).max(0);
    cv.text(row, start, s, class);
}

fn truncate(s: &str, max: usize) -> String {
    if UnicodeWidthStr::width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthStr::width(ch.to_string().as_str());
        if w + cw + 1 > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    if max > 0 {
        out.push('…');
    }
    out
}
