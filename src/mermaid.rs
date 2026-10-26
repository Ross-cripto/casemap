use crate::model::{Diagram, Direction, Edge, LineStyle, Mark, Node, Shape};

/// Mermaid flowchart source for `diagram`. Marks become `class` and `linkStyle`
/// lines, which GitHub colours and casemap's parser ignores.
pub fn write(diagram: &Diagram) -> String {
    let direction = match diagram.direction {
        Direction::TopDown => "TD",
        Direction::BottomUp => "BT",
        Direction::LeftRight => "LR",
        Direction::RightLeft => "RL",
    };
    let mut output = format!("flowchart {direction}\n");
    // Declare nodes before membership so parsing preserves their original indexes.
    for (index, node) in diagram.nodes.iter().enumerate() {
        let (open, close) = shape_delimiters(node.shape);
        let label = node_label(node);
        output.push_str(&format!("    n{index}{open}\"{label}\"{close}\n"));
    }
    write_members(diagram, None, 1, &mut output);
    for edge in &diagram.edges {
        write_edge(edge, &mut output);
    }
    write_marks(diagram, &mut output);
    output
}

fn escape_label(label: &str, edge: bool) -> String {
    let mut output = String::new();
    for character in label.chars() {
        match character {
            '#' => output.push_str("#35;"),
            '"' => output.push_str("#quot;"),
            '<' => output.push_str("#lt;"),
            '>' => output.push_str("#gt;"),
            '`' => output.push_str("#96;"),
            '|' if edge => output.push_str("#124;"),
            _ => output.push(character),
        }
    }
    output
}

fn shape_delimiters(shape: Shape) -> (&'static str, &'static str) {
    match shape {
        Shape::Rect => ("[", "]"),
        Shape::Round => ("(", ")"),
        Shape::Stadium => ("([", "])"),
        Shape::Circle => ("((", "))"),
        Shape::Diamond => ("{", "}"),
        Shape::Hexagon => ("{{", "}}"),
        Shape::Database => ("[(", ")]"),
        Shape::Subroutine => ("[[", "]]"),
    }
}

fn node_label(node: &Node) -> String {
    node.label
        .iter()
        .map(|line| escape_label(line, false))
        .collect::<Vec<_>>()
        .join("<br/>")
}

fn write_members(diagram: &Diagram, parent: Option<usize>, depth: usize, output: &mut String) {
    let indent = "    ".repeat(depth);
    if parent.is_some() {
        for (index, membership) in diagram.node_cluster.iter().enumerate() {
            if *membership == parent {
                output.push_str(&format!("{indent}n{index}\n"));
            }
        }
    }
    for (index, cluster) in diagram.clusters.iter().enumerate() {
        if cluster.parent != parent {
            continue;
        }
        let title = escape_label(&cluster.title, false);
        output.push_str(&format!("{indent}subgraph c{index} [\"{title}\"]\n"));
        write_members(diagram, Some(index), depth + 1, output);
        output.push_str(&format!("{indent}end\n"));
    }
}

fn edge_operator(style: LineStyle, arrow_from: bool, arrow_to: bool) -> &'static str {
    match style {
        LineStyle::Solid => {
            if arrow_from && arrow_to {
                "<-->"
            } else if arrow_from || arrow_to {
                "-->"
            } else {
                "---"
            }
        }
        LineStyle::Dotted => {
            if arrow_from && arrow_to {
                "<-.->"
            } else if arrow_from || arrow_to {
                "-.->"
            } else {
                "-.-"
            }
        }
        LineStyle::Thick => {
            if arrow_from && arrow_to {
                "<==>"
            } else if arrow_from || arrow_to {
                "==>"
            } else {
                "==="
            }
        }
    }
}

fn write_edge(edge: &Edge, output: &mut String) {
    let (from, to) = if edge.arrow_from && !edge.arrow_to {
        (edge.to, edge.from)
    } else {
        (edge.from, edge.to)
    };
    let operator = edge_operator(edge.style, edge.arrow_from, edge.arrow_to);
    let label = edge
        .label
        .as_ref()
        .map(|label| format!("|\"{}\"|", escape_label(label, true)))
        .unwrap_or_default();
    output.push_str(&format!("    n{from} {operator}{label} n{to}\n"));
}

struct MarkStyle {
    class: &'static str,
    node: &'static str,
    link: &'static str,
}

fn mark_style(mark: Mark) -> Option<MarkStyle> {
    match mark {
        Mark::Same => None,
        Mark::Added => Some(MarkStyle {
            class: "added",
            node: "fill:#dcfce7,stroke:#16a34a,color:#14532d",
            link: "stroke:#16a34a,stroke-width:2px",
        }),
        Mark::Removed => Some(MarkStyle {
            class: "removed",
            node: "fill:#fee2e2,stroke:#dc2626,color:#7f1d1d,stroke-dasharray:4 3",
            link: "stroke:#dc2626,stroke-width:2px,stroke-dasharray:4 3",
        }),
        Mark::Changed => Some(MarkStyle {
            class: "changed",
            node: "fill:#fef3c7,stroke:#d97706,color:#78350f",
            link: "stroke:#d97706,stroke-width:2px",
        }),
    }
}

fn write_marks(diagram: &Diagram, output: &mut String) {
    let mut classes = String::new();
    let mut links = String::new();
    for mark in [Mark::Added, Mark::Removed, Mark::Changed] {
        let Some(style) = mark_style(mark) else {
            continue;
        };
        let mut ids = diagram
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.mark == mark)
            .map(|(index, _)| format!("n{index}"))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        let edges = diagram
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.mark == mark)
            .map(|(index, _)| index.to_string())
            .collect::<Vec<_>>();
        if !ids.is_empty() || !edges.is_empty() {
            output.push_str(&format!("classDef {} {}\n", style.class, style.node));
        }
        if !ids.is_empty() {
            classes.push_str(&format!("class {} {}\n", ids.join(","), style.class));
        }
        if !edges.is_empty() {
            links.push_str(&format!("linkStyle {} {}\n", edges.join(","), style.link));
        }
    }
    output.push_str(&classes);
    output.push_str(&links);
}
