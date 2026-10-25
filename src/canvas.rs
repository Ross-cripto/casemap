//! Character canvas. Lines are stored as per-cell connection flags tagged with
//! their owner, then resolved to box-drawing glyphs at output time, so
//! junctions, corners and attachments come out right regardless of draw order.
//! Two different edges crossing at right angles render as a "hop" (the
//! vertical passes over) rather than a `┼`, which would read as a junction.

use unicode_width::UnicodeWidthChar;

use crate::model::Mark;

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Cluster(u32),
    Node(u32),
    Edge(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pen {
    Square,
    Round,
    Heavy,
    Dashed,
    Faint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    NodeBorder,
    NodeText,
    Edge,
    Arrow,
    EdgeLabel,
    ClusterBorder,
    ClusterTitle,
}

#[derive(Clone, Default)]
struct Cell {
    text: Option<(char, Class, Mark)>,
    continuation: bool,
    lines: Vec<(Owner, u8, Pen, Mark)>,
}

pub struct Canvas {
    rows: i32,
    cols: i32,
    cells: Vec<Cell>,
    ink: Mark,
}

impl Canvas {
    pub fn new(rows: i32, cols: i32) -> Self {
        let size = (rows.max(0) * cols.max(0)) as usize;
        Canvas {
            rows,
            cols,
            cells: vec![Cell::default(); size],
            ink: Mark::Same,
        }
    }

    pub fn set_ink(&mut self, mark: Mark) {
        self.ink = mark;
    }

    fn cell_mut(&mut self, r: i32, c: i32) -> Option<&mut Cell> {
        if r < 0 || c < 0 || r >= self.rows || c >= self.cols {
            return None;
        }
        Some(&mut self.cells[(r * self.cols + c) as usize])
    }

    /// Draws a horizontal or vertical line between two cells, inclusive.
    pub fn line(&mut self, r0: i32, c0: i32, r1: i32, c1: i32, owner: Owner, pen: Pen) {
        if r0 == r1 {
            let (a, b) = (c0.min(c1), c0.max(c1));
            for c in a..=b {
                let f = if c > a { LEFT } else { 0 } | if c < b { RIGHT } else { 0 };
                self.mark(r0, c, owner, f, pen);
            }
        } else if c0 == c1 {
            let (a, b) = (r0.min(r1), r0.max(r1));
            for r in a..=b {
                let f = if r > a { UP } else { 0 } | if r < b { DOWN } else { 0 };
                self.mark(r, c0, owner, f, pen);
            }
        }
    }

    fn mark(&mut self, r: i32, c: i32, owner: Owner, flags: u8, pen: Pen) {
        if flags == 0 {
            return;
        }
        let ink = self.ink;
        if let Some(cell) = self.cell_mut(r, c) {
            match cell.lines.iter_mut().find(|e| e.0 == owner) {
                Some(e) => e.1 |= flags,
                None => cell.lines.push((owner, flags, pen, ink)),
            }
        }
    }

    pub fn put(&mut self, r: i32, c: i32, ch: char, class: Class) {
        let w = ch.width().unwrap_or(1).max(1) as i32;
        let ink = self.ink;
        if let Some(cell) = self.cell_mut(r, c) {
            cell.text = Some((ch, class, ink));
            cell.continuation = false;
        }
        if w == 2 {
            if let Some(cell) = self.cell_mut(r, c + 1) {
                cell.text = None;
                cell.continuation = true;
            }
        }
    }

    /// Writes a character only where no node or edge line occupies the cell.
    pub fn put_soft(&mut self, r: i32, c: i32, ch: char, class: Class) {
        let free = match self.cell_mut(r, c) {
            Some(cell) => cell.lines.iter().all(|e| matches!(e.0, Owner::Cluster(_))),
            None => false,
        };
        if free {
            self.put(r, c, ch, class);
        }
    }

    /// Writes text left to right starting at (r, c).
    pub fn text(&mut self, r: i32, c: i32, s: &str, class: Class) {
        let mut col = c;
        for ch in s.chars() {
            self.put(r, col, ch, class);
            col += ch.width().unwrap_or(1).max(1) as i32;
        }
    }

    pub fn render(&self, ascii: bool, color: bool) -> String {
        let mut out = String::new();
        let mut lines: Vec<String> = Vec::with_capacity(self.rows as usize);
        for r in 0..self.rows {
            let row: Vec<_> = (0..self.cols)
                .map(|c| {
                    let cell = &self.cells[(r * self.cols + c) as usize];
                    (resolve(cell), cell.continuation)
                })
                .collect();
            let last = row
                .iter()
                .rposition(|(t, cont)| *cont || t.is_some_and(|(ch, _, _)| ch != ' '));
            let mut line = String::new();
            if let Some(last) = last {
                let mut cur: Option<(Class, Mark)> = None;
                for (t, cont) in &row[..=last] {
                    if *cont {
                        continue;
                    }
                    match t {
                        None => {
                            if color && cur.is_some() {
                                line.push_str("\x1b[0m");
                                cur = None;
                            }
                            line.push(' ');
                        }
                        Some((ch, class, mark)) => {
                            if color && cur != Some((*class, *mark)) {
                                line.push_str("\x1b[0m");
                                line.push_str(sgr(*class, *mark));
                                cur = Some((*class, *mark));
                            }
                            line.push(if ascii { to_ascii(*ch) } else { *ch });
                        }
                    }
                }
                if color && cur.is_some() {
                    line.push_str("\x1b[0m");
                }
            }
            lines.push(line);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
        out
    }
}

fn resolve(cell: &Cell) -> Option<(char, Class, Mark)> {
    if let Some(t) = cell.text {
        return Some(t);
    }
    if cell.lines.is_empty() {
        return None;
    }
    let is_cluster = |o: &Owner| matches!(o, Owner::Cluster(_));
    let solid: Vec<&(Owner, u8, Pen, Mark)> = if cell.lines.iter().any(|e| !is_cluster(&e.0)) {
        cell.lines.iter().filter(|e| !is_cluster(&e.0)).collect()
    } else {
        cell.lines.iter().collect()
    };
    let node = solid.iter().find(|e| matches!(e.0, Owner::Node(_)));
    let edges: Vec<&&(Owner, u8, Pen, Mark)> = solid
        .iter()
        .filter(|e| matches!(e.0, Owner::Edge(_)))
        .collect();

    if node.is_none() && edges.len() >= 2 {
        let vertical = UP | DOWN;
        let horizontal = LEFT | RIGHT;
        let all_straight = edges.iter().all(|e| e.1 == vertical || e.1 == horizontal);
        if all_straight {
            if let Some(v) = edges.iter().find(|e| e.1 == vertical) {
                if edges.iter().any(|e| e.1 == horizontal) {
                    return Some((glyph(vertical, v.2), Class::Edge, v.3));
                }
            }
        }
    }

    let flags = solid.iter().fold(0, |a, e| a | e.1);
    let (pen, class, mark) = match node {
        Some(n) => (n.2, Class::NodeBorder, n.3),
        None if !edges.is_empty() => {
            let p = edges[0].2;
            (
                if edges.iter().all(|e| e.2 == p) {
                    p
                } else {
                    Pen::Round
                },
                Class::Edge,
                shared_edge_mark(&edges),
            )
        }
        None => (solid[0].2, Class::ClusterBorder, Mark::Same),
    };
    Some((glyph(flags, pen), class, mark))
}

fn shared_edge_mark(edges: &[&&(Owner, u8, Pen, Mark)]) -> Mark {
    if edges.iter().all(|edge| edge.3 == edges[0].3) {
        edges[0].3
    } else {
        Mark::Same
    }
}

const LIGHT: [char; 16] = [
    ' ', '╵', '╷', '│', '╴', '┘', '┐', '┤', '╶', '└', '┌', '├', '─', '┴', '┬', '┼',
];
const HEAVY: [char; 16] = [
    ' ', '╹', '╻', '┃', '╸', '┛', '┓', '┫', '╺', '┗', '┏', '┣', '━', '┻', '┳', '╋',
];

fn glyph(flags: u8, pen: Pen) -> char {
    let f = (flags & 15) as usize;
    if pen == Pen::Heavy {
        return HEAVY[f];
    }
    let rounded = pen != Pen::Square;
    match f {
        3 => match pen {
            Pen::Dashed => '┆',
            Pen::Faint => '┊',
            _ => '│',
        },
        12 => match pen {
            Pen::Dashed => '┄',
            Pen::Faint => '┈',
            _ => '─',
        },
        5 if rounded => '╯',
        6 if rounded => '╮',
        9 if rounded => '╰',
        10 if rounded => '╭',
        _ => LIGHT[f],
    }
}

const ADDED: [&str; 3] = ["\x1b[1;32m", "\x1b[3;32m", "\x1b[32m"];
const REMOVED: [&str; 3] = ["\x1b[1;31m", "\x1b[3;31m", "\x1b[31m"];
const CHANGED: [&str; 3] = ["\x1b[1;33m", "\x1b[3;33m", "\x1b[33m"];
const BOLD_WEIGHT: usize = 0;
const ITALIC_WEIGHT: usize = 1;
const PLAIN_WEIGHT: usize = 2;

fn class_weight(class: Class) -> usize {
    match class {
        Class::NodeText => BOLD_WEIGHT,
        Class::EdgeLabel => ITALIC_WEIGHT,
        Class::NodeBorder
        | Class::Edge
        | Class::Arrow
        | Class::ClusterBorder
        | Class::ClusterTitle => PLAIN_WEIGHT,
    }
}

fn marked_palette(mark: Mark) -> Option<&'static [&'static str; 3]> {
    match mark {
        Mark::Same => None,
        Mark::Added => Some(&ADDED),
        Mark::Removed => Some(&REMOVED),
        Mark::Changed => Some(&CHANGED),
    }
}

pub fn mark_sgr(mark: Mark) -> Option<&'static str> {
    marked_palette(mark).map(|palette| palette[PLAIN_WEIGHT])
}

/// Colours chosen to read well on both dark and light terminal themes.
fn sgr(class: Class, mark: Mark) -> &'static str {
    match marked_palette(mark) {
        Some(palette) => palette[class_weight(class)],
        None => match class {
            Class::NodeBorder => "",
            Class::NodeText => "\x1b[1m",
            Class::Edge => "\x1b[90m",
            Class::Arrow => "\x1b[90m",
            Class::EdgeLabel => "\x1b[3;90m",
            Class::ClusterBorder => "\x1b[90m",
            Class::ClusterTitle => "\x1b[1;90m",
        },
    }
}

fn to_ascii(ch: char) -> char {
    match ch {
        '│' | '┃' | '┆' | '┊' | '╵' | '╷' | '╹' | '╻' => '|',
        '─' | '━' | '┄' | '┈' | '╴' | '╶' | '╸' | '╺' => '-',
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╰' | '╯' | '┏' | '┓'
        | '┗' | '┛' | '┣' | '┫' | '┳' | '┻' | '╋' => '+',
        '▼' => 'v',
        '▲' => '^',
        '▶' => '>',
        '◀' => '<',
        '╱' => '/',
        '╲' => '\\',
        '↻' => '@',
        other => other,
    }
}
