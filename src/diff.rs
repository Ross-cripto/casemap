use crate::model::{Diagram, Mark};

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    StepAdded {
        id: String,
        label: String,
    },
    StepRemoved {
        id: String,
        label: String,
    },
    StepChanged {
        id: String,
        old: String,
        new: String,
        shape: bool,
    },
    LinkAdded {
        from: String,
        to: String,
        label: Option<String>,
    },
    LinkRemoved {
        from: String,
        to: String,
        label: Option<String>,
    },
    LinkChanged {
        from: String,
        to: String,
        old: Option<String>,
        new: Option<String>,
        style_changed: bool,
        arrows_changed: bool,
    },
}

impl Change {
    pub fn mark(&self) -> Mark {
        match self {
            Self::StepAdded { .. } | Self::LinkAdded { .. } => Mark::Added,
            Self::StepRemoved { .. } | Self::LinkRemoved { .. } => Mark::Removed,
            Self::StepChanged { .. } | Self::LinkChanged { .. } => Mark::Changed,
        }
    }
}

#[derive(Debug)]
pub struct DiagramDiff {
    pub union: Diagram,
    pub changes: Vec<Change>,
}

fn restore_cluster(old: &Diagram, union: &mut Diagram, index: usize) -> usize {
    let cluster = &old.clusters[index];
    if let Some(existing) = union
        .clusters
        .iter()
        .position(|candidate| candidate.id == cluster.id)
    {
        return existing;
    }
    let parent = cluster
        .parent
        .map(|parent| restore_cluster(old, union, parent));
    let mut restored = cluster.clone();
    restored.parent = parent;
    let restored_index = union.clusters.len();
    union.clusters.push(restored);
    restored_index
}

fn same_link(old: &crate::model::Edge, new: &crate::model::Edge) -> bool {
    old.label == new.label
        && old.style == new.style
        && old.arrow_from == new.arrow_from
        && old.arrow_to == new.arrow_to
}

pub fn diff(old: &Diagram, new: &Diagram) -> DiagramDiff {
    let mut union = new.clone();
    let mut changes = Vec::new();
    classify_nodes(old, &mut union, &mut changes);
    append_removed_nodes(old, new, &mut union, &mut changes);
    let matched = classify_edges(old, new, &mut union, &mut changes);
    append_removed_edges(old, &matched, &mut union, &mut changes);
    apply_markers(&mut union);
    DiagramDiff { union, changes }
}

fn classify_nodes(old: &Diagram, union: &mut Diagram, changes: &mut Vec<Change>) {
    for node in &mut union.nodes {
        match old.nodes.iter().find(|candidate| candidate.id == node.id) {
            Some(previous) if previous.label == node.label && previous.shape == node.shape => {
                node.mark = Mark::Same
            }
            Some(previous) => {
                node.mark = Mark::Changed;
                changes.push(Change::StepChanged {
                    id: node.id.clone(),
                    old: previous.label.join(" "),
                    new: node.label.join(" "),
                    shape: previous.shape != node.shape,
                });
            }
            None => {
                node.mark = Mark::Added;
                changes.push(Change::StepAdded {
                    id: node.id.clone(),
                    label: node.label.join(" "),
                });
            }
        }
    }
}

fn append_removed_nodes(
    old: &Diagram,
    new: &Diagram,
    union: &mut Diagram,
    changes: &mut Vec<Change>,
) {
    for (index, node) in old.nodes.iter().enumerate() {
        if new.nodes.iter().any(|candidate| candidate.id == node.id) {
            continue;
        }
        let cluster = old.node_cluster[index].map(|cluster| restore_cluster(old, union, cluster));
        let mut removed = node.clone();
        removed.mark = Mark::Removed;
        changes.push(Change::StepRemoved {
            id: node.id.clone(),
            label: node.label.join(" "),
        });
        union.nodes.push(removed);
        union.node_cluster.push(cluster);
    }
}

fn classify_edges(
    old: &Diagram,
    new: &Diagram,
    union: &mut Diagram,
    changes: &mut Vec<Change>,
) -> Vec<bool> {
    let mut matched = vec![false; old.edges.len()];
    for edge in &mut union.edges {
        let from = &new.nodes[edge.from].id;
        let to = &new.nodes[edge.to].id;
        let previous = old.edges.iter().enumerate().find(|(index, candidate)| {
            !matched[*index]
                && old.nodes[candidate.from].id == *from
                && old.nodes[candidate.to].id == *to
        });
        match previous {
            Some((index, previous)) => {
                matched[index] = true;
                if same_link(previous, edge) {
                    edge.mark = Mark::Same;
                } else {
                    edge.mark = Mark::Changed;
                    changes.push(Change::LinkChanged {
                        from: from.clone(),
                        to: to.clone(),
                        old: previous.label.clone(),
                        new: edge.label.clone(),
                        style_changed: previous.style != edge.style,
                        arrows_changed: previous.arrow_from != edge.arrow_from
                            || previous.arrow_to != edge.arrow_to,
                    });
                }
            }
            None => {
                edge.mark = Mark::Added;
                changes.push(Change::LinkAdded {
                    from: from.clone(),
                    to: to.clone(),
                    label: edge.label.clone(),
                });
            }
        }
    }
    matched
}

fn append_removed_edges(
    old: &Diagram,
    matched: &[bool],
    union: &mut Diagram,
    changes: &mut Vec<Change>,
) {
    for (index, edge) in old.edges.iter().enumerate() {
        if matched[index] {
            continue;
        }
        let from = &old.nodes[edge.from].id;
        let to = &old.nodes[edge.to].id;
        let mut removed = edge.clone();
        if let (Some(from_index), Some(to_index)) = (
            union.nodes.iter().position(|node| node.id == *from),
            union.nodes.iter().position(|node| node.id == *to),
        ) {
            removed.from = from_index;
            removed.to = to_index;
            removed.mark = Mark::Removed;
            union.edges.push(removed);
            changes.push(Change::LinkRemoved {
                from: from.clone(),
                to: to.clone(),
                label: edge.label.clone(),
            });
        }
    }
}

fn apply_markers(union: &mut Diagram) {
    for node in &mut union.nodes {
        if let (Some(symbol), Some(label)) = (node.mark.symbol(), node.label.first_mut()) {
            *label = format!("{symbol} {label}");
        }
    }
    for edge in &mut union.edges {
        if let Some(symbol) = edge.mark.symbol() {
            edge.label = Some(match &edge.label {
                Some(label) => format!("{symbol} {label}"),
                None => symbol.to_string(),
            });
        }
    }
}

/// A missing base flow is compared against an empty diagram in the same direction.
pub fn all_added(diagram: &Diagram) -> DiagramDiff {
    let empty = Diagram {
        direction: diagram.direction,
        nodes: Vec::new(),
        edges: Vec::new(),
        clusters: Vec::new(),
        node_cluster: Vec::new(),
    };
    diff(&empty, diagram)
}

fn quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            character if character.is_control() => quoted.extend(character.escape_default()),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn label_suffix(label: &Option<String>) -> String {
    match label {
        Some(label) => format!(" {}", quote(label)),
        None => String::new(),
    }
}

impl DiagramDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut steps_added = 0;
        let mut steps_removed = 0;
        let mut steps_changed = 0;
        let mut links_added = 0;
        let mut links_removed = 0;
        let mut links_changed = 0;
        for change in &self.changes {
            match change {
                Change::StepAdded { .. } => steps_added += 1,
                Change::StepRemoved { .. } => steps_removed += 1,
                Change::StepChanged { .. } => steps_changed += 1,
                Change::LinkAdded { .. } => links_added += 1,
                Change::LinkRemoved { .. } => links_removed += 1,
                Change::LinkChanged { .. } => links_changed += 1,
            }
        }
        let mut parts = Vec::new();
        for (symbol, count, noun) in [
            ('+', steps_added, "step"),
            ('-', steps_removed, "step"),
            ('~', steps_changed, "step"),
            ('+', links_added, "link"),
            ('-', links_removed, "link"),
            ('~', links_changed, "link"),
        ] {
            if count > 0 {
                let plural = if count == 1 { "" } else { "s" };
                parts.push(format!("{symbol}{count} {noun}{plural}"));
            }
        }
        if parts.is_empty() {
            "no changes".to_string()
        } else {
            parts.join(", ")
        }
    }

    pub fn details(&self) -> Vec<(Mark, String)> {
        self.changes
            .iter()
            .map(|change| (change.mark(), change_detail(change)))
            .collect()
    }
}

fn change_detail(change: &Change) -> String {
    match change {
        Change::StepAdded { id, label } => format!("+ step {id} {}", quote(label)),
        Change::StepRemoved { id, label } => format!("- step {id} {}", quote(label)),
        Change::StepChanged {
            id,
            old,
            new,
            shape,
        } => {
            let suffix = if *shape { " (shape)" } else { "" };
            format!("~ step {id} {} -> {}{suffix}", quote(old), quote(new))
        }
        Change::LinkAdded { from, to, label } => {
            format!("+ link {from} -> {to}{}", label_suffix(label))
        }
        Change::LinkRemoved { from, to, label } => {
            format!("- link {from} -> {to}{}", label_suffix(label))
        }
        Change::LinkChanged {
            from,
            to,
            old,
            new,
            style_changed,
            arrows_changed,
        } => {
            let mut prefix = format!("~ link {from} -> {to}");
            if old != new {
                prefix.push_str(&format!("{} ->{}", label_suffix(old), label_suffix(new)));
            }
            let mut parts = Vec::new();
            if *arrows_changed {
                parts.push("arrows");
            }
            if *style_changed {
                parts.push("style");
            }
            if !parts.is_empty() {
                prefix.push_str(&format!(" ({})", parts.join(", ")));
            }
            prefix
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{flows, parser};

    fn detail_text(difference: &DiagramDiff) -> Vec<String> {
        difference
            .details()
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }

    fn checkout() -> (Diagram, Diagram) {
        let old = flows::parse_flow(
            "checkout.md",
            include_str!("../tests/fixtures/flows/before/checkout.md"),
        );
        let new = flows::parse_flow(
            "checkout.md",
            include_str!("../tests/fixtures/flows/after/checkout.md"),
        );
        (
            flows::diagram(&old).unwrap().unwrap(),
            flows::diagram(&new).unwrap().unwrap(),
        )
    }

    #[test]
    fn identical_diagrams_have_no_changes() {
        let diagram = parser::parse("graph TD\na[A] -->|yes| b[B]").unwrap();
        let result = diff(&diagram, &diagram);
        assert!(result.is_empty());
        assert_eq!(result.summary(), "no changes");
        assert!(result
            .union
            .nodes
            .iter()
            .all(|node| node.mark == Mark::Same));
        assert!(result
            .union
            .edges
            .iter()
            .all(|edge| edge.mark == Mark::Same));
        assert_eq!(result.union.nodes[0].label, ["A"]);
        assert_eq!(result.union.edges[0].label.as_deref(), Some("yes"));
    }

    #[test]
    fn classifies_steps_and_links() {
        let (old, new) = checkout();
        let result = diff(&old, &new);
        for (id, mark, label) in [
            ("fraud", Mark::Added, "+ Fraud check"),
            ("address", Mark::Removed, "- Enter address"),
            ("confirm", Mark::Changed, "~ Order confirmed and emailed"),
        ] {
            let node = result
                .union
                .nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap();
            assert_eq!(node.mark, mark);
            assert_eq!(node.label[0], label);
        }
        assert_eq!(result.changes.len(), 10);
        for edge in &result.union.edges {
            if let Some(symbol) = edge.mark.symbol() {
                assert!(edge.label.as_ref().unwrap().starts_with(symbol));
            }
        }
    }

    #[test]
    fn duplicate_links_match_in_declaration_order() {
        let old = parser::parse("graph TD\na --> b\na --> b").unwrap();
        let new = parser::parse("graph TD\na --> b").unwrap();
        let result = diff(&old, &new);
        assert_eq!(result.union.edges[0].mark, Mark::Same);
        assert_eq!(result.union.edges[1].mark, Mark::Removed);
        assert!(matches!(
            result.changes.as_slice(),
            [Change::LinkRemoved { .. }]
        ));
    }

    #[test]
    fn removed_node_keeps_its_cluster() {
        let old =
            parser::parse("graph TD\nsubgraph parent[Outer]\nsubgraph s[Inner]\na[A]\nend\nend")
                .unwrap();
        let new = parser::parse("graph LR\nb[B]").unwrap();
        let result = diff(&old, &new);
        let cluster = result.union.node_cluster[1].unwrap();
        assert_eq!(result.union.clusters[cluster].id, "s");
        assert_eq!(
            result.union.clusters[result.union.clusters[cluster].parent.unwrap()].id,
            "parent"
        );
        assert_eq!(result.union.direction, new.direction);
    }

    #[test]
    fn details_keep_unicode_and_apostrophes() {
        let old = parser::parse("graph TD\na[Validación]").unwrap();
        let new = parser::parse("graph TD\na[Validación del pago]\nb[Don't pay]").unwrap();
        assert_eq!(
            detail_text(&diff(&old, &new)),
            [
                "~ step a \"Validación\" -> \"Validación del pago\"",
                "+ step b \"Don't pay\"",
            ]
        );
    }

    #[test]
    fn link_changed_names_what_changed() {
        let old = parser::parse("graph TD\na --> b").unwrap();
        for (source, expected) in [
            ("graph TD\na <--> b", "~ link a -> b (arrows)"),
            ("graph TD\na -.-> b", "~ link a -> b (style)"),
            ("graph TD\na <-.-> b", "~ link a -> b (arrows, style)"),
        ] {
            let new = parser::parse(source).unwrap();
            assert_eq!(detail_text(&diff(&old, &new)), [expected]);
        }
    }

    #[test]
    fn label_and_style_changes_are_both_named() {
        let old = parser::parse("graph TD\na -->|y| b").unwrap();
        let new = parser::parse("graph TD\na -.->|x| b").unwrap();
        assert_eq!(
            detail_text(&diff(&old, &new)),
            ["~ link a -> b \"y\" -> \"x\" (style)"]
        );
    }

    #[test]
    fn summary_and_details_text() {
        let (old, new) = checkout();
        let result = diff(&old, &new);
        assert_eq!(
            result.summary(),
            "+1 step, -1 step, ~1 step, +3 links, -3 links, ~1 link"
        );
        assert_eq!(
            detail_text(&result),
            [
                "+ step fraud \"Fraud check\"",
                "~ step confirm \"Order confirmed\" -> \"Order confirmed and emailed\"",
                "- step address \"Enter address\"",
                "+ link cart -> pay",
                "+ link pay -> fraud \"yes\"",
                "+ link fraud -> confirm",
                "~ link pay -> error \"no\" -> \"declined\"",
                "- link cart -> address",
                "- link address -> pay",
                "- link pay -> confirm \"yes\"",
            ]
        );
        assert!(result.details().iter().all(|(_, line)| line.is_ascii()));
    }
}
