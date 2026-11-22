//! Core diagram model shared by the parser, comparison and image layout.

use crate::icons::Icon;

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
    pub icon: Option<Icon>,
    pub mark: Mark,
}

impl Node {
    pub fn effective_icon(&self) -> Icon {
        self.icon.unwrap_or(match self.shape {
            Shape::Stadium => Icon::User,
            Shape::Diamond => Icon::Diamond,
            _ => Icon::CircleDot,
        })
    }
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
}
