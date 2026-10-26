use std::{fs, path::Path};

use casemap::{
    Diagram, Direction, Mark, diff, flows, mermaid,
    model::{LineStyle, Shape},
    parser,
};

#[derive(Debug, PartialEq, Eq)]
struct NodeSummary {
    label: Vec<String>,
    shape: Shape,
    cluster: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct EdgeSummary {
    from: usize,
    to: usize,
    label: Option<String>,
    style: LineStyle,
    arrow_from: bool,
    arrow_to: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct ClusterSummary {
    title: String,
    parent: Option<usize>,
}

fn node_summaries(diagram: &Diagram) -> Vec<NodeSummary> {
    diagram
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| NodeSummary {
            label: node.label.clone(),
            shape: node.shape,
            cluster: diagram.node_cluster[index],
        })
        .collect()
}

fn edge_summaries(diagram: &Diagram) -> Vec<EdgeSummary> {
    diagram
        .edges
        .iter()
        .map(|edge| {
            let reverse = edge.arrow_from && !edge.arrow_to;
            EdgeSummary {
                from: if reverse { edge.to } else { edge.from },
                to: if reverse { edge.from } else { edge.to },
                label: edge.label.clone(),
                style: edge.style,
                arrow_from: edge.arrow_from && edge.arrow_to,
                arrow_to: edge.arrow_from || edge.arrow_to,
            }
        })
        .collect()
}

fn cluster_summaries(diagram: &Diagram) -> Vec<ClusterSummary> {
    diagram
        .clusters
        .iter()
        .map(|cluster| ClusterSummary {
            title: cluster.title.clone(),
            parent: cluster.parent,
        })
        .collect()
}

fn assert_same_diagram(expected: &Diagram, actual: &Diagram) {
    assert_eq!(actual.direction, expected.direction);
    assert_eq!(node_summaries(actual), node_summaries(expected));
    assert_eq!(edge_summaries(actual), edge_summaries(expected));
    assert_eq!(cluster_summaries(actual), cluster_summaries(expected));
    for (index, node) in actual.nodes.iter().enumerate() {
        assert_eq!(node.id, format!("n{index}"));
    }
    for (index, cluster) in actual.clusters.iter().enumerate() {
        assert_eq!(cluster.id, format!("c{index}"));
    }
}

fn assert_round_trip(diagram: &Diagram) {
    let source = mermaid::write(diagram);
    let actual = parser::parse(&source).unwrap_or_else(|error| panic!("{error}\n{source}"));
    assert_same_diagram(diagram, &actual);
}

fn fixture_diagrams() -> Vec<Diagram> {
    let mut diagrams = Vec::new();
    for directory in ["examples", "tests/fixtures"] {
        let initial_count = diagrams.len();
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "mmd") {
                diagrams.push(parser::parse(&fs::read_to_string(path).unwrap()).unwrap());
            }
        }
        assert!(diagrams.len() > initial_count, "no diagrams in {directory}");
    }
    diagrams
}

fn catalog_diagrams() -> Vec<Diagram> {
    let before = flows::load_dir(Path::new("tests/fixtures/flows/before")).unwrap();
    let after = flows::load_dir(Path::new("tests/fixtures/flows/after")).unwrap();
    assert!(before.len() >= 3);
    assert!(after.len() >= 3);
    let mut diagrams = Vec::new();
    for flow in before.iter().chain(&after) {
        diagrams.push(flows::diagram(flow).unwrap().unwrap());
    }
    for change in flows::compare(&before, &after) {
        let expected = match &change {
            flows::FlowChange::Modified { diff, .. } | flows::FlowChange::New { diff, .. } => {
                diff::unmarked(&diff.union)
            }
            flows::FlowChange::Removed { flow } => {
                let old = flows::diagram(flow).unwrap().unwrap();
                let empty = Diagram {
                    direction: old.direction,
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    clusters: Vec::new(),
                    node_cluster: Vec::new(),
                };
                diff::unmarked(&diff::diff(&old, &empty).union)
            }
            flows::FlowChange::Unchanged { .. } => continue,
            flows::FlowChange::Broken { error, .. } => panic!("{error}"),
        };
        let report = casemap::check::Report {
            flows: 1,
            problems: Vec::new(),
        };
        let shipped = flows::render_markdown(&flows::MarkdownInput {
            changes: &[change],
            stale: &[],
            since: "main",
            base: "abcdef123",
            report: &report,
            catalog: "docs/flows",
            uncovered: &[],
        });
        let source = shipped
            .split("\n```mermaid\n")
            .nth(1)
            .unwrap()
            .split("\n```")
            .next()
            .unwrap();
        let actual = parser::parse(source).unwrap();
        assert_same_diagram(&expected, &actual);
        diagrams.push(expected);
    }
    diagrams
}

#[test]
fn mermaid_round_trips_every_diagram() {
    let mut diagrams = fixture_diagrams();
    diagrams.extend(catalog_diagrams());
    assert!(diagrams.iter().any(|diagram| !diagram.clusters.is_empty()));
    assert!(
        diagrams
            .iter()
            .any(|diagram| diagram.nodes.iter().any(|node| node.mark != Mark::Same))
    );
    for mut diagram in diagrams {
        for direction in [
            Direction::TopDown,
            Direction::BottomUp,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            diagram.direction = direction;
            assert_round_trip(&diagram);
        }
    }
}

#[test]
fn writes_every_shape_with_quoted_labels() {
    for syntax in [
        "[\"Label\"]",
        "(\"Label\")",
        "([\"Label\"])",
        "((\"Label\"))",
        "{\"Label\"}",
        "{{\"Label\"}}",
        "[(\"Label\")]",
        "[[\"Label\"]]",
    ] {
        let diagram = parser::parse(&format!("flowchart TD\na{syntax}")).unwrap();
        assert_eq!(
            mermaid::write(&diagram),
            format!("flowchart TD\n    n0{syntax}\n")
        );
        assert_round_trip(&diagram);
    }
}

#[test]
fn writes_each_operator_and_reverses_source_only_arrows() {
    for operator in [
        "-->", "---", "<-->", "-.->", "-.-", "<-.->", "==>", "===", "<==>",
    ] {
        let diagram = parser::parse(&format!("flowchart TD\na {operator}|\"edge\"| b")).unwrap();
        assert!(mermaid::write(&diagram).contains(&format!("n0 {operator}|\"edge\"| n1")));
        assert_round_trip(&diagram);
    }
    for (style, operator) in [
        (LineStyle::Solid, "-->"),
        (LineStyle::Dotted, "-.->"),
        (LineStyle::Thick, "==>"),
    ] {
        let mut diagram = parser::parse("graph TD\na --> b").unwrap();
        diagram.edges[0].style = style;
        diagram.edges[0].arrow_from = true;
        diagram.edges[0].arrow_to = false;
        assert!(mermaid::write(&diagram).contains(&format!("n1 {operator} n0")));
        assert_round_trip(&diagram);
    }
}

#[test]
fn escapes_labels_and_preserves_literal_entities() {
    let mut diagram = parser::parse("graph TD\na[A] --> b[B]").unwrap();
    diagram.nodes[0].label = vec!["Say \"yes\" & <no> #quot; `fence` #;".into(), "Next".into()];
    diagram.edges[0].label = Some("\"yes|no\" #124;".into());
    let source = mermaid::write(&diagram);
    assert!(source.contains("Say #quot;yes#quot;"));
    assert!(source.contains("<br/>Next"));
    assert!(source.contains("#124;no"));
    assert_round_trip(&diagram);
}

#[test]
fn nests_clusters_and_preserves_membership_and_titles() {
    let diagram = parser::parse("graph RL\nsubgraph outer [\"Outside\"]\na[A]\nsubgraph inner [\"Inside\"]\nb[B]\nend\nend\nc[C]\na --> b\nb --> c").unwrap();
    assert_round_trip(&diagram);
    let mut quoted = diagram;
    quoted.clusters[0].title = "A \"title\" #quot;".into();
    assert_round_trip(&quoted);
}

#[test]
fn mermaid_marks_map_exactly_to_classes_and_link_styles() {
    let mut diagram =
        parser::parse("graph TD\nz --> a\na --> b\nb --> c\nc --> z\nz --> b").unwrap();
    for (node, mark) in
        diagram
            .nodes
            .iter_mut()
            .zip([Mark::Added, Mark::Added, Mark::Removed, Mark::Changed])
    {
        node.mark = mark;
    }
    for (edge, mark) in diagram.edges.iter_mut().zip([
        Mark::Same,
        Mark::Added,
        Mark::Removed,
        Mark::Changed,
        Mark::Added,
    ]) {
        edge.mark = mark;
    }
    let source = mermaid::write(&diagram);
    let styling: Vec<_> = source
        .lines()
        .filter(|line| line.starts_with("class") || line.starts_with("linkStyle"))
        .collect();
    assert_eq!(
        styling,
        [
            "classDef added fill:#dcfce7,stroke:#16a34a,color:#14532d",
            "classDef removed fill:#fee2e2,stroke:#dc2626,color:#7f1d1d,stroke-dasharray:4 3",
            "classDef changed fill:#fef3c7,stroke:#d97706,color:#78350f",
            "class n0,n1 added",
            "class n2 removed",
            "class n3 changed",
            "linkStyle 1,4 stroke:#16a34a,stroke-width:2px",
            "linkStyle 2 stroke:#dc2626,stroke-width:2px,stroke-dasharray:4 3",
            "linkStyle 3 stroke:#d97706,stroke-width:2px",
        ]
    );
    for node in &mut diagram.nodes {
        node.mark = Mark::Same;
    }
    for edge in &mut diagram.edges {
        edge.mark = Mark::Same;
    }
    assert!(!mermaid::write(&diagram).contains("classDef"));
}

#[test]
fn preserves_decoded_break_text_and_literal_backslashes() {
    let diagram = parser::parse("graph TD\na[\"#lt;br/#gt;#92;n#quot;\"] --> b[B]").unwrap();
    assert_round_trip(&diagram);
}

#[test]
fn only_used_class_defs_are_emitted() {
    for (mark, class) in [
        (Mark::Added, "added"),
        (Mark::Removed, "removed"),
        (Mark::Changed, "changed"),
    ] {
        let mut diagram = parser::parse("graph TD\na --> b").unwrap();
        diagram.edges[0].mark = mark;
        let source = mermaid::write(&diagram);
        assert_eq!(source.matches("classDef ").count(), 1);
        assert!(source.contains(&format!("classDef {class} ")));
        assert!(!source.contains("class "));
        diagram.nodes[0].mark = mark;
        assert!(mermaid::write(&diagram).contains(&format!("class n0 {class}")));
    }
}

#[test]
fn replaces_reserved_and_spaced_ids() {
    let diagram = parser::parse(
        "graph TD\nsubgraph spaced cluster [\"Title\"]\ndefault[Default] --> end[End]\nend",
    )
    .unwrap();
    let source = mermaid::write(&diagram);
    assert!(source.contains("subgraph c0 [\"Title\"]"));
    assert!(source.contains("n0 --> n1"));
    assert_round_trip(&diagram);
}
