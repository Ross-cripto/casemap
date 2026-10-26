//! Item sizing. Node boxes are sized from their label and shape, then grown
//! so every port gets its own cell with a gap between neighbouring ports.
//! The extent carrying the centre port is kept odd so centres are exact.

use unicode_width::UnicodeWidthStr;

use super::{ItemKind, Layout};
use crate::model::{Node, Shape};

/// Screen size (width, height) of a node box before port adjustments.
pub fn node_box(node: &Node) -> (i32, i32) {
    let tw = node
        .label
        .iter()
        .map(|l| UnicodeWidthStr::width(l.as_str()))
        .max()
        .unwrap_or(0) as i32;
    let lines = node.label.len().max(1) as i32;
    match node.shape {
        Shape::Rect | Shape::Round | Shape::Stadium | Shape::Circle => (tw + 4, lines + 2),
        Shape::Subroutine | Shape::Diamond | Shape::Hexagon => (tw + 6, lines + 2),
        Shape::Database => (tw + 4, lines + 3),
    }
}

/// Port cells lost at each end of a side, beyond the corner itself.
pub fn port_inset(shape: Shape) -> i32 {
    match shape {
        Shape::Diamond | Shape::Hexagon | Shape::Subroutine => 1,
        _ => 0,
    }
}

fn odd(x: i32) -> i32 {
    if x % 2 == 0 { x + 1 } else { x }
}

pub fn assign(lay: &mut Layout, d: &crate::model::Diagram) {
    let vertical = lay.direction.is_vertical();
    for i in 0..lay.items.len() {
        let (ext_o, ext_r) = match &lay.items[i].kind {
            ItemKind::Node(n) => {
                let node = &d.nodes[*n];
                let (w, h) = node_box(node);
                let ports = (lay.out_units(d, i).len()).max(lay.in_segs[i].len()) as i32;
                let need = 2 * ports + 1;
                if vertical {
                    (odd(w.max(need + 2 * port_inset(node.shape))), h)
                } else {
                    // A database lid takes one row of the side away from ports.
                    let lid = i32::from(node.shape == Shape::Database);
                    (odd(h.max(need + lid)), w)
                }
            }
            ItemKind::Dummy | ItemKind::LoopTurn => (1, 0),
            ItemKind::Label(t) => {
                let tw = UnicodeWidthStr::width(t.as_str()) as i32 + 2;
                if vertical { (odd(tw), 1) } else { (1, tw) }
            }
            ItemKind::Border { .. } => (1, 0),
        };
        lay.items[i].ext_o = ext_o;
        lay.items[i].ext_r = ext_r;
    }
}
