//! Invariants every rendered diagram must satisfy.

use std::fs;

use strata::{Direction, Options, parser};

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["examples", "tests/fixtures"] {
        for entry in fs::read_dir(dir).expect("fixture dir") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "mmd") {
                let src = fs::read_to_string(&path).expect("read fixture");
                out.push((path.display().to_string(), src));
            }
        }
    }
    assert!(!out.is_empty());
    out
}

fn all_directions() -> [Option<Direction>; 5] {
    [
        None,
        Some(Direction::TopDown),
        Some(Direction::BottomUp),
        Some(Direction::LeftRight),
        Some(Direction::RightLeft),
    ]
}

#[test]
fn one_arrowhead_per_arrow_end() {
    for (name, src) in sources() {
        let mut d = parser::parse(&src).unwrap();
        d.fold_self_loops();
        let expected: usize = d
            .edges
            .iter()
            .map(|e| usize::from(e.arrow_to) + usize::from(e.arrow_from))
            .sum();
        for dir in all_directions() {
            let out = strata::render_diagram(
                &d,
                &Options {
                    direction: dir,
                    ..Options::default()
                },
            );
            let found = out
                .chars()
                .filter(|c| matches!(c, '▼' | '▲' | '▶' | '◀'))
                .count();
            assert_eq!(found, expected, "{name} {dir:?}\n{out}");
        }
    }
}

#[test]
fn every_label_is_visible() {
    for (name, src) in sources() {
        let d = parser::parse(&src).unwrap();
        for dir in all_directions() {
            let out = strata::render_diagram(
                &d,
                &Options {
                    direction: dir,
                    ..Options::default()
                },
            );
            for node in &d.nodes {
                for line in &node.label {
                    let line = line.trim_end_matches(" ↻");
                    assert!(
                        out.contains(line),
                        "{name} {dir:?}: missing '{line}'\n{out}"
                    );
                }
            }
            for edge in &d.edges {
                if let Some(label) = &edge.label {
                    assert!(
                        out.contains(label.as_str()),
                        "{name} {dir:?}: missing edge label '{label}'\n{out}"
                    );
                }
            }
        }
    }
}

#[test]
fn no_dangling_line_ends() {
    // Half-cell glyphs only appear when a line stops without meeting anything.
    for (name, src) in sources() {
        for dir in all_directions() {
            let out = strata::render_str(
                &src,
                &Options {
                    direction: dir,
                    ..Options::default()
                },
            )
            .unwrap();
            let bad: Vec<char> = out
                .chars()
                .filter(|c| matches!(c, '╵' | '╷' | '╴' | '╶' | '╹' | '╻' | '╸' | '╺'))
                .collect();
            assert!(bad.is_empty(), "{name} {dir:?}: dangling {bad:?}\n{out}");
        }
    }
}

#[test]
fn rendering_is_deterministic() {
    for (name, src) in sources() {
        let a = strata::render_str(&src, &Options::default()).unwrap();
        let b = strata::render_str(&src, &Options::default()).unwrap();
        assert_eq!(a, b, "{name}");
    }
}

#[test]
fn ascii_mode_is_pure_ascii() {
    let src = fs::read_to_string("examples/architecture.mmd").unwrap();
    let out = strata::render_str(
        &src,
        &Options {
            ascii: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(out.is_ascii(), "{out}");
}

#[test]
fn handles_degenerate_inputs() {
    for src in [
        "graph TD",
        "graph TD\n  a",
        "graph LR\n  a --> a",
        "flowchart TD\n subgraph empty\n end",
    ] {
        strata::render_str(src, &Options::default()).unwrap();
    }
}
