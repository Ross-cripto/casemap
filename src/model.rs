//! Core diagram model shared by the parser, the layout engine and the renderer.

/// Direction in which ranks (layers) advance across the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

impl Direction {
    pub fn parse(s: &str) -> Option<Direction> {
        match s.trim().to_ascii_uppercase().as_str() {
            "TD" | "TB" => Some(Direction::TopDown),
            "BT" => Some(Direction::BottomUp),
            "LR" => Some(Direction::LeftRight),
            "RL" => Some(Direction::RightLeft),
            _ => None,
        }
    }

    /// True when ranks advance along the screen's vertical axis.
    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::TopDown | Direction::BottomUp)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Circle,
    Diamond,
    Hexagon,
    Database,
    Subroutine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LineStyle {
    Solid,
    Dotted,
    Thick,
}

/// Parsed elements are `Same`; the diff module assigns the other states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mark {
    #[default]
    Same,
    Added,
    Removed,
    Changed,
}

impl Mark {
    pub fn symbol(self) -> Option<char> {
        match self {
            Self::Same => None,
            Self::Added => Some('+'),
            Self::Removed => Some('-'),
            Self::Changed => Some('~'),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub label: Vec<String>,
    pub shape: Shape,
    pub mark: Mark,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub style: LineStyle,
    pub arrow_from: bool,
    pub arrow_to: bool,
    pub mark: Mark,
}

#[derive(Clone, Debug)]
pub struct Cluster {
    pub id: String,
    pub title: String,
    pub parent: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Diagram {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
    /// Innermost cluster each node belongs to.
    pub node_cluster: Vec<Option<usize>>,
}

impl Diagram {
    /// Cluster ancestry, outermost first, ending with `cluster` itself.
    pub fn cluster_path(&self, cluster: Option<usize>) -> Vec<usize> {
        let mut path = Vec::new();
        let mut cur = cluster;
        while let Some(c) = cur {
            path.push(c);
            cur = self.clusters[c].parent;
        }
        path.reverse();
        path
    }

    /// Self-loops have no good orthogonal drawing in a layered layout, so they
    /// are folded into a marker on the node label instead.
    pub fn fold_self_loops(&mut self) {
        let edges = std::mem::take(&mut self.edges);
        for e in edges {
            if e.from == e.to {
                if let Some(last) = self.nodes[e.from].label.last_mut() {
                    if !last.ends_with('↻') {
                        last.push_str(" ↻");
                    }
                }
            } else {
                self.edges.push(e);
            }
        }
    }
}
