#[cfg_attr(not(feature = "cli"), allow(unused_imports))]
mod common;

use casemap::{Direction, Options, diff, flows};
use common::TempRepo;
use std::fs;
use std::path::Path;

const INVALID_UTF8: [u8; 2] = [0xff, 0xfe];

fn pair() -> (Vec<flows::Flow>, Vec<flows::Flow>) {
    (
        flows::load_dir(Path::new("tests/fixtures/flows/before")).unwrap(),
        flows::load_dir(Path::new("tests/fixtures/flows/after")).unwrap(),
    )
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
fn union_render_invariants() {
    let (old, new) = pair();
    let mut differences = Vec::new();
    for name in ["checkout", "login"] {
        let previous = old.iter().find(|flow| flow.name == name).unwrap();
        let current = new.iter().find(|flow| flow.name == name).unwrap();
        let previous = flows::diagram(previous).unwrap().unwrap();
        let current = flows::diagram(current).unwrap().unwrap();
        differences.push(diff::diff(&previous, &current));
    }
    for (catalog, name) in [(&new, "refund"), (&old, "legacy-export")] {
        let flow = catalog.iter().find(|flow| flow.name == name).unwrap();
        differences.push(diff::all_added(&flows::diagram(flow).unwrap().unwrap()));
    }
    const EXPECTED_UNION_CASES: usize = 4;
    assert_eq!(differences.len(), EXPECTED_UNION_CASES);
    for difference in differences {
        let union = &difference.union;
        let mut folded = union.clone();
        folded.fold_self_loops();
        let expected: usize = folded
            .edges
            .iter()
            .map(|edge| usize::from(edge.arrow_to) + usize::from(edge.arrow_from))
            .sum();
        for direction in all_directions() {
            let opts = Options {
                direction,
                ..Options::default()
            };
            let output = casemap::render_diagram(union, &opts);
            let arrows = output
                .chars()
                .filter(|ch| matches!(ch, '▼' | '▲' | '▶' | '◀'))
                .count();
            assert_eq!(arrows, expected, "{direction:?}\n{output}");
            let dangling: Vec<char> = output
                .chars()
                .filter(|ch| matches!(ch, '╵' | '╷' | '╴' | '╶' | '╹' | '╻' | '╸' | '╺'))
                .collect();
            assert!(dangling.is_empty(), "{direction:?}: {dangling:?}\n{output}");
            assert_eq!(output, casemap::render_diagram(union, &opts));
            for node in &union.nodes {
                for label in &node.label {
                    assert!(
                        output.contains(label),
                        "{direction:?}: missing {label}\n{output}"
                    );
                }
            }
            for edge in &union.edges {
                if let Some(label) = &edge.label {
                    assert!(
                        output.contains(label),
                        "{direction:?}: missing {label}\n{output}"
                    );
                }
            }
            let ascii = casemap::render_diagram(
                union,
                &Options {
                    ascii: true,
                    ..opts
                },
            );
            assert!(ascii.is_ascii(), "{direction:?}\n{ascii}");
        }
    }
}

#[test]
fn colour_follows_marks() {
    let (old, new) = pair();
    let old = flows::diagram(&old[0]).unwrap().unwrap();
    let new = flows::diagram(&new[0]).unwrap().unwrap();
    let output = casemap::render_diagram(
        &diff::diff(&old, &new).union,
        &Options {
            color: true,
            ..Options::default()
        },
    );
    const ADDED_INK: &str = "\x1b[1;32m";
    const REMOVED_INK: &str = "\x1b[1;31m";
    const CHANGED_INK: &str = "\x1b[1;33m";
    const SAME_INK: &str = "\x1b[1m";
    for (ink, text) in [
        (ADDED_INK, "+ Fraud check"),
        (REMOVED_INK, "- Enter address"),
        (CHANGED_INK, "~ Order confirmed and emailed"),
        (SAME_INK, "Customer opens cart"),
    ] {
        assert!(
            output.contains(&format!("{ink}{text}")),
            "missing {ink}{text}\n{output}"
        );
    }
    for dir in ["examples", "tests/fixtures"] {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "mmd") {
                let source = fs::read_to_string(&path).unwrap();
                let output = casemap::render_str(
                    &source,
                    &Options {
                        color: true,
                        ..Options::default()
                    },
                )
                .unwrap();
                for ink in ["32m", "31m", "33m"] {
                    assert!(
                        !output.contains(ink),
                        "{}: unexpected {ink}\n{output}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn markdown_renders_each_block_with_heading() {
    let text = "# A\nProse\n```mermaid\ngraph TD\na[First] --> b[Second]\n```\n## B\n```bash\n# not a heading\n```\n```mermaid\ngraph LR\nc[Third] --> d[Fourth]\n```\n";
    let output = casemap::render_markdown(text, &Options::default()).unwrap();
    for expected in ["A\n", "B\n", "First", "Second", "Third", "Fourth"] {
        assert!(output.contains(expected), "missing {expected}\n{output}");
    }
    assert!(!output.contains("not a heading"));
    let error = casemap::render_markdown(
        &text.replace("c[Third] --> d[Fourth]", "c -->"),
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(error.line, 13);
    assert_eq!(
        casemap::render_markdown("# Empty", &Options::default()).unwrap(),
        ""
    );
}

#[test]
fn buses_group_only_edges_with_the_same_mark() {
    let mut diagram = casemap::parser::parse("graph TD\na --> b\na --> c\na --> d").unwrap();
    diagram.edges[0].mark = casemap::Mark::Added;
    diagram.edges[1].mark = casemap::Mark::Removed;
    diagram.edges[2].mark = casemap::Mark::Added;
    let layout = casemap::layout::layout(&diagram);
    let source = layout
        .items
        .iter()
        .position(|item| matches!(item.kind, casemap::layout::ItemKind::Node(0)))
        .unwrap();
    let units = layout.out_units(&diagram, source);
    assert_eq!(units.len(), 2);
    assert_eq!(units[0].mark, casemap::Mark::Added);
    assert_eq!(units[0].segs.len(), 2);
    assert_eq!(units[1].mark, casemap::Mark::Removed);
    assert_eq!(units[1].segs.len(), 1);
}

#[cfg(unix)]
#[test]
fn load_dir_skips_hidden_and_dangling_entries() {
    use std::os::unix::fs::symlink;
    let repo = TempRepo::new();
    fs::copy(
        "tests/fixtures/flows/after/checkout.md",
        repo.file("checkout.md"),
    )
    .unwrap();
    fs::write(repo.file(".hidden.md"), "hidden").unwrap();
    symlink("/nonexistent", repo.file(".#checkout.md")).unwrap();
    symlink("/nonexistent", repo.file("stale.md")).unwrap();
    symlink("checkout.md", repo.file("link.md")).unwrap();
    fs::create_dir(repo.file("nested.md")).unwrap();
    let catalog = flows::load_dir(repo.path()).unwrap();
    let names: Vec<_> = catalog.iter().map(|flow| flow.name.as_str()).collect();
    assert_eq!(names, ["checkout", "link"]);
}

#[test]
fn load_dir_errors_name_the_file() {
    let repo = TempRepo::new();
    fs::write(repo.file("bad.md"), INVALID_UTF8).unwrap();
    let error = flows::load_dir(repo.path()).unwrap_err();
    assert!(error.to_string().contains("bad.md"), "{error}");
}

#[test]
fn read_catalog_classifies_entries() {
    let repo = TempRepo::new();
    fs::create_dir(repo.flow_path("nested")).unwrap();
    for name in ["notes.txt", "README.md", "readme.MD", ".hidden.md"] {
        fs::write(repo.flow_path(name), "ignored").unwrap();
    }
    fs::write(repo.flow_path("bad.md"), INVALID_UTF8).unwrap();
    repo.add_fixture("after", "checkout.md");
    let entries = flows::read_catalog(&repo.file(common::FLOWS_DIR)).unwrap();
    assert_eq!(entries.len(), 4);
    assert!(matches!(&entries[0], flows::Entry::Unreadable { name, .. } if name == "bad.md"));
    assert!(
        matches!(&entries[1], flows::Entry::Flow { name, text } if name == "checkout.md" && text.starts_with("# Checkout"))
    );
    assert!(matches!(&entries[2], flows::Entry::Ignored { name } if name == "nested"));
    assert!(matches!(&entries[3], flows::Entry::Ignored { name } if name == "notes.txt"));
    assert_eq!(
        flows::load_dir(&repo.file(common::FLOWS_DIR))
            .unwrap_err()
            .path,
        repo.flow_path("bad.md")
    );
}
