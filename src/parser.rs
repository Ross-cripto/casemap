//! Parser for the Mermaid flowchart subset casemap understands.
//!
//! Supported: `flowchart`/`graph` headers with TD/TB/BT/LR/RL, node shapes
//! (`[]`, `()`, `([])`, `[[]]`, `[()]`, `(())`, `{}`, `{{}}`, `>]`, `[/ /]`),
//! quoted labels, `<br>` line breaks, chained links (`A --> B --> C`),
//! `&` fan-out, solid/dotted/thick links with `|label|` or inline labels,
//! bidirectional links, and nested `subgraph ... end` blocks.
//! Styling statements (`classDef`, `style`, `linkStyle`, `click`, ...) are ignored.

use std::collections::HashMap;

use crate::model::{Cluster, Diagram, Direction, Edge, LineStyle, Mark, Node, Shape};

#[derive(Debug, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

pub fn parse(source: &str) -> Result<Diagram, ParseError> {
    let mut b = Builder::default();
    let mut header_seen = false;
    for (idx, raw) in source.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        for stmt in split_statements(trimmed) {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }
            if !header_seen {
                header_seen = true;
                if let Some(dir) = parse_header(stmt) {
                    b.direction = Some(dir);
                    continue;
                }
            }
            b.statement(stmt).map_err(|message| ParseError {
                line: line_no,
                message,
            })?;
        }
    }
    Ok(b.finish())
}

pub fn is_header(stmt: &str) -> bool {
    parse_header(stmt).is_some()
}

pub fn first_statement(source: &str) -> Option<(usize, &str)> {
    source.lines().enumerate().find_map(|(index, line)| {
        let statement = line.trim();
        if statement.is_empty() || statement.starts_with("%%") {
            None
        } else {
            Some((index + 1, statement))
        }
    })
}

fn parse_header(stmt: &str) -> Option<Direction> {
    let mut words = stmt.split_whitespace();
    let kw = words.next()?.to_ascii_lowercase();
    if kw != "flowchart" && kw != "graph" {
        return None;
    }
    Some(
        words
            .next()
            .and_then(Direction::parse)
            .unwrap_or(Direction::TopDown),
    )
}

/// Splits a line on `;` outside quotes, brackets and `|label|` pipes.
pub(crate) fn split_statements(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut in_pipe = false;
    for ch in line.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            '|' if !in_quote => in_pipe = !in_pipe,
            '[' | '(' | '{' if !in_quote && !in_pipe => depth += 1,
            ']' | ')' | '}' if !in_quote && !in_pipe => depth -= 1,
            ';' if !in_quote && !in_pipe && depth <= 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    out.push(cur);
    out
}

/// id tokens of a statement, ignoring text inside `[] () {} "" ||`.
pub(crate) fn id_tokens_outside_labels(statement: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut in_pipe = false;
    let mut characters = statement.char_indices().peekable();
    while let Some((start, character)) = characters.next() {
        match character {
            '"' => in_quote = !in_quote,
            '|' if !in_quote => in_pipe = !in_pipe,
            '[' | '(' | '{' if !in_quote && !in_pipe => depth += 1,
            ']' | ')' | '}' if !in_quote && !in_pipe => depth -= 1,
            _ => {}
        }
        let inside_label = depth > 0 || in_quote || in_pipe;
        if inside_label || !is_id_char(character) {
            continue;
        }
        let mut end = start + character.len_utf8();
        while let Some(&(position, next)) = characters.peek() {
            let joiner = matches!(next, '-' | '.')
                && statement[position + next.len_utf8()..]
                    .chars()
                    .next()
                    .is_some_and(is_id_char);
            if !is_id_char(next) && !joiner {
                break;
            }
            end = position + next.len_utf8();
            characters.next();
        }
        tokens.push(&statement[start..end]);
    }
    tokens
}

pub(crate) fn statement_mentions_id(statement: &str, id: &str) -> bool {
    id_tokens_outside_labels(statement).contains(&id)
}

#[derive(Default)]
struct Builder {
    direction: Option<Direction>,
    nodes: Vec<Node>,
    index: HashMap<String, usize>,
    edges: Vec<Edge>,
    clusters: Vec<Cluster>,
    node_cluster: Vec<Option<usize>>,
    stack: Vec<usize>,
}

struct Link {
    style: LineStyle,
    arrow_from: bool,
    arrow_to: bool,
    label: Option<String>,
}

impl Builder {
    fn finish(self) -> Diagram {
        Diagram {
            direction: self.direction.unwrap_or(Direction::TopDown),
            nodes: self.nodes,
            edges: self.edges,
            clusters: self.clusters,
            node_cluster: self.node_cluster,
        }
    }

    fn statement(&mut self, stmt: &str) -> Result<(), String> {
        let (first, rest) = match stmt.find(char::is_whitespace) {
            Some(i) => (&stmt[..i], stmt[i..].trim()),
            None => (stmt, ""),
        };
        match first.to_ascii_lowercase().as_str() {
            "subgraph" => {
                self.open_subgraph(rest);
                Ok(())
            }
            "end" if rest.is_empty() => {
                self.stack.pop();
                Ok(())
            }
            "direction" | "classdef" | "class" | "style" | "linkstyle" | "click" | "acctitle"
            | "accdescr" => Ok(()),
            _ => self.chain(stmt),
        }
    }

    fn open_subgraph(&mut self, rest: &str) {
        let rest = rest.trim();
        let (id, title) = if let Some(open) = rest.find('[') {
            let id = rest[..open].trim().to_string();
            let inner = rest[open + 1..].trim_end_matches(']');
            (id, unquote(inner.trim()))
        } else if rest.starts_with('"') {
            let t = unquote(rest);
            (t.clone(), t)
        } else {
            (rest.to_string(), rest.to_string())
        };
        let parent = self.stack.last().copied();
        let idx = self.clusters.len();
        let id = if id.is_empty() {
            format!("subgraph{idx}")
        } else {
            id
        };
        let title = if title.is_empty() { id.clone() } else { title };
        self.clusters.push(Cluster { id, title, parent });
        self.stack.push(idx);
    }

    fn chain(&mut self, stmt: &str) -> Result<(), String> {
        let mut cur = Cursor::new(stmt);
        let mut prev: Option<Vec<usize>> = None;
        let mut pending: Option<Link> = None;
        loop {
            cur.skip_ws();
            if cur.eof() {
                break;
            }
            let group = self.node_group(&mut cur)?;
            if let (Some(p), Some(link)) = (&prev, pending.take()) {
                for &a in p {
                    for &b in &group {
                        self.edges.push(Edge {
                            from: a,
                            to: b,
                            label: link.label.clone(),
                            style: link.style,
                            arrow_from: link.arrow_from,
                            arrow_to: link.arrow_to,
                            mark: Mark::Same,
                        });
                    }
                }
            }
            prev = Some(group);
            cur.skip_ws();
            if cur.eof() {
                break;
            }
            match parse_link(&mut cur) {
                Some(link) => pending = Some(link),
                None => return Err(format!("unexpected input near '{}'", cur.rest())),
            }
        }
        if pending.is_some() {
            return Err("link has no target node".into());
        }
        Ok(())
    }

    fn node_group(&mut self, cur: &mut Cursor) -> Result<Vec<usize>, String> {
        let mut group = vec![self.node_ref(cur)?];
        loop {
            let save = cur.pos;
            cur.skip_ws();
            if cur.peek() == Some('&') {
                cur.advance(1);
                group.push(self.node_ref(cur)?);
            } else {
                cur.pos = save;
                return Ok(group);
            }
        }
    }

    fn node_ref(&mut self, cur: &mut Cursor) -> Result<usize, String> {
        cur.skip_ws();
        let id = cur.take_id();
        if id.is_empty() {
            return Err(format!("expected a node id near '{}'", cur.rest()));
        }
        let shape = parse_shape(cur)?;
        if cur.starts_with(":::") {
            cur.advance(3);
            cur.take_id();
        }
        Ok(self.touch_node(id, shape))
    }

    fn touch_node(&mut self, id: String, shape: Option<(Shape, String)>) -> usize {
        let current = self.stack.last().copied();
        if let Some(&i) = self.index.get(&id) {
            if let Some((shape, text)) = shape {
                self.nodes[i].shape = shape;
                self.nodes[i].label = split_label(&text);
            }
            if self.node_cluster[i].is_none() {
                self.node_cluster[i] = current;
            }
            return i;
        }
        let (shape, label) = match shape {
            Some((s, text)) => (s, split_label(&text)),
            None => (Shape::Rect, vec![id.clone()]),
        };
        let i = self.nodes.len();
        self.index.insert(id.clone(), i);
        self.nodes.push(Node {
            id,
            label,
            shape,
            mark: Mark::Same,
        });
        self.node_cluster.push(current);
        i
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn split_label(text: &str) -> Vec<String> {
    let mut t = text.to_string();
    for br in ["<br/>", "<br />", "<br>", "\\n"] {
        t = t.replace(br, "\n");
    }
    let lines: Vec<String> = t.split('\n').map(|l| l.trim().to_string()).collect();
    if lines.iter().all(|l| l.is_empty()) {
        vec![String::new()]
    } else {
        lines
    }
}

const SHAPES: &[(&str, &str, Shape)] = &[
    ("([", "])", Shape::Stadium),
    ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Database),
    ("((", "))", Shape::Circle),
    ("{{", "}}", Shape::Hexagon),
    ("[/", "]", Shape::Rect),
    ("[\\", "]", Shape::Rect),
    ("(", ")", Shape::Round),
    ("[", "]", Shape::Rect),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Rect),
];

fn parse_shape(cur: &mut Cursor) -> Result<Option<(Shape, String)>, String> {
    for &(open, close, shape) in SHAPES {
        if !cur.starts_with(open) {
            continue;
        }
        cur.advance(open.chars().count());
        let text = if cur.peek() == Some('"') {
            cur.advance(1);
            let end = cur.find("\"").ok_or("unterminated quoted label")?;
            let t = cur.slice(end);
            cur.advance(end + 1);
            cur.skip_ws();
            if !cur.starts_with(close) {
                return Err(format!("expected '{close}' after quoted label"));
            }
            t
        } else {
            let end = cur
                .find(close)
                .ok_or_else(|| format!("missing '{close}'"))?;
            let t = cur.slice(end);
            cur.advance(end);
            t
        };
        cur.advance(close.chars().count());
        let text = text.trim().trim_end_matches(['/', '\\']).trim().to_string();
        return Ok(Some((shape, text)));
    }
    Ok(None)
}

fn parse_link(cur: &mut Cursor) -> Option<Link> {
    let save = cur.pos;
    let mut arrow_from = false;
    if cur.peek() == Some('<') {
        arrow_from = true;
        cur.advance(1);
    }
    let style = match (cur.peek(), cur.peek_at(1)) {
        (Some('-'), Some('.')) => LineStyle::Dotted,
        (Some('-'), Some('-')) => LineStyle::Solid,
        (Some('='), Some('=')) => LineStyle::Thick,
        _ => {
            cur.pos = save;
            return None;
        }
    };
    let mut label = None;
    let arrow_to;
    match style {
        LineStyle::Solid | LineStyle::Thick => {
            let ch = if style == LineStyle::Solid { '-' } else { '=' };
            let run = cur.count_run(ch);
            let head = cur.eat_head();
            if run == 2 && !head && cur.peek().is_some_and(char::is_whitespace) {
                let closer: String = [ch, ch].iter().collect();
                match cur.find(&closer) {
                    Some(end) => {
                        label = Some(cur.slice(end).trim().to_string());
                        cur.advance(end);
                        cur.count_run(ch);
                        arrow_to = cur.eat_head();
                    }
                    None => arrow_to = false,
                }
            } else {
                arrow_to = head;
            }
        }
        LineStyle::Dotted => {
            cur.advance(1); // '-'
            cur.count_run('.');
            let dash = cur.peek() == Some('-');
            if dash {
                cur.advance(1);
            }
            let head = cur.eat_head();
            if !dash && !head && cur.peek().is_some_and(char::is_whitespace) {
                match cur.find(".-") {
                    Some(end) => {
                        label = Some(cur.slice(end).trim().to_string());
                        cur.advance(end);
                        cur.count_run('.');
                        if cur.peek() == Some('-') {
                            cur.advance(1);
                        }
                        arrow_to = cur.eat_head();
                    }
                    None => arrow_to = false,
                }
            } else {
                arrow_to = head;
            }
        }
    }
    let save_pipe = cur.pos;
    cur.skip_ws();
    if cur.peek() == Some('|') {
        cur.advance(1);
        if let Some(end) = cur.find("|") {
            label = Some(cur.slice(end).trim().to_string());
            cur.advance(end + 1);
        }
    } else {
        cur.pos = save_pipe;
    }
    let label = label.filter(|l| !l.is_empty()).map(|l| unquote(&l));
    Some(Link {
        style,
        arrow_from,
        arrow_to,
        label,
    })
}

struct Cursor {
    chars: Vec<char>,
    pos: usize,
}

impl Cursor {
    fn new(s: &str) -> Self {
        Cursor {
            chars: s.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.chars.get(self.pos + k).copied()
    }

    fn eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.chars.len());
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c))
    }

    /// Offset (from the cursor) of the next occurrence of `pat`.
    fn find(&self, pat: &str) -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        let rest = &self.chars[self.pos..];
        if p.len() > rest.len() {
            return None;
        }
        (0..=rest.len() - p.len()).find(|&i| rest[i..i + p.len()] == p[..])
    }

    fn slice(&self, len: usize) -> String {
        self.chars[self.pos..self.pos + len].iter().collect()
    }

    fn rest(&self) -> String {
        self.chars[self.pos..].iter().collect()
    }

    fn count_run(&mut self, ch: char) -> usize {
        let start = self.pos;
        while self.peek() == Some(ch) {
            self.pos += 1;
        }
        self.pos - start
    }

    /// Consumes an arrowhead (`>`, or `x`/`o` when standalone).
    fn eat_head(&mut self) -> bool {
        match self.peek() {
            Some('>') => {
                self.pos += 1;
                true
            }
            Some('x') | Some('o') if self.peek_at(1).is_none_or(|c| !is_id_char(c)) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn take_id(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            let joiner = (c == '-' || c == '.')
                && self.pos > start
                && self.peek_at(1).is_some_and(is_id_char);
            if is_id_char(c) || joiner {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.chars[start..self.pos].iter().collect()
    }
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chains_shapes_and_labels() {
        let d = parse(
            "flowchart LR\n  a([Start]) --> b{Ok?} -->|yes| c[(DB)]\n  b -. retry .-> a\n  b == fast ==> d",
        )
        .unwrap();
        assert_eq!(d.direction, Direction::LeftRight);
        assert_eq!(d.nodes.len(), 4);
        assert_eq!(d.nodes[1].shape, Shape::Diamond);
        assert_eq!(d.nodes[2].shape, Shape::Database);
        assert_eq!(d.edges.len(), 4);
        assert_eq!(d.edges[1].label.as_deref(), Some("yes"));
        assert_eq!(d.edges[2].style, LineStyle::Dotted);
        assert_eq!(d.edges[2].label.as_deref(), Some("retry"));
        assert_eq!(d.edges[3].style, LineStyle::Thick);
        assert_eq!(d.edges[3].label.as_deref(), Some("fast"));
    }

    #[test]
    fn parses_subgraphs_and_fanout() {
        let d =
            parse("graph TD\nsubgraph billing [Billing]\n  api-gw --> svc\nend\nx & y --> api-gw")
                .unwrap();
        assert_eq!(d.clusters.len(), 1);
        assert_eq!(d.clusters[0].title, "Billing");
        assert_eq!(d.node_cluster[0], Some(0));
        assert_eq!(d.nodes[0].id, "api-gw");
        assert_eq!(d.edges.len(), 3);
    }
}
