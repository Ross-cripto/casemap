use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use casemap::quality::{Metrics, measure};
use casemap::{Diagram, Direction, flows, parser};

const MINIMUM_DIAGRAMS: usize = 29 + 10 + 1;

fn collect_files(directory: &Path, extension: &str, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read diagram directory") {
        let path = entry.expect("read diagram entry").path();
        if path.extension().is_some_and(|value| value == extension) {
            paths.push(path);
        }
    }
}

fn diagrams() -> Vec<(String, Diagram)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    for entry in fs::read_dir(root.join("tests/fixtures/corpus")).expect("read corpus") {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            collect_files(&path, "md", &mut paths);
        }
    }
    collect_files(&root.join("docs/flows"), "md", &mut paths);
    collect_files(&root.join("examples"), "mmd", &mut paths);
    paths.sort();
    assert!(paths.len() >= MINIMUM_DIAGRAMS);
    paths
        .into_iter()
        .map(|path| {
            let name = path.strip_prefix(root).unwrap().display().to_string();
            let source = fs::read_to_string(&path).expect("read diagram");
            let parsed = if path.extension().is_some_and(|value| value == "md") {
                let flow = flows::parse_flow(&name, &source);
                flows::diagram(&flow).unwrap_or_else(|| panic!("{name}: missing diagram"))
            } else {
                parser::parse(&source)
            };
            let diagram = parsed.unwrap_or_else(|error| panic!("{name}: {error}"));
            (name, diagram)
        })
        .collect()
}

fn table(rows: &[(String, Metrics)]) -> String {
    let path_width = rows.iter().map(|(path, _)| path.len()).max().unwrap_or(4);
    let mut output = format!(
        "{:<path_width$}  {:>5} {:>6} {:>9} {:>5} {:>9} {:>13} {:>12} {:>10}\n",
        "path",
        "width",
        "height",
        "crossings",
        "bends",
        "label_gap",
        "max_label_gap",
        "extra_layers",
        "back_edges"
    );
    let mut totals = Metrics::default();
    for (path, metrics) in rows {
        write_row(&mut output, path, metrics, path_width);
        totals.width += metrics.width;
        totals.height += metrics.height;
        totals.crossings += metrics.crossings;
        totals.bends += metrics.bends;
        totals.label_gap += metrics.label_gap;
        totals.max_label_gap = totals.max_label_gap.max(metrics.max_label_gap);
        totals.extra_layers += metrics.extra_layers;
        totals.back_edges += metrics.back_edges;
    }
    write_row(&mut output, "TOTAL", &totals, path_width);
    output
}

fn write_row(output: &mut String, path: &str, metrics: &Metrics, path_width: usize) {
    writeln!(
        output,
        "{path:<path_width$}  {:>5} {:>6} {:>9} {:>5} {:>9} {:>13} {:>12} {:>10}",
        metrics.width,
        metrics.height,
        metrics.crossings,
        metrics.bends,
        metrics.label_gap,
        metrics.max_label_gap,
        metrics.extra_layers,
        metrics.back_edges
    )
    .unwrap();
}

#[test]
fn corpus() {
    let rows: Vec<_> = diagrams()
        .into_iter()
        .map(|(path, diagram)| (path, measure(&diagram)))
        .collect();
    let report = table(&rows);
    println!("{report}");
    insta::assert_snapshot!(report);
}

#[test]
fn metrics_are_deterministic() {
    for (path, diagram) in diagrams() {
        assert_eq!(measure(&diagram), measure(&diagram), "{path}");
    }
}

#[test]
fn metrics_on_known_shapes() {
    for direction in [
        Direction::TopDown,
        Direction::BottomUp,
        Direction::LeftRight,
        Direction::RightLeft,
    ] {
        let mut chain = parser::parse("graph TD\n A --> B --> C").unwrap();
        chain.direction = direction;
        let metrics = measure(&chain);
        assert_eq!(metrics.crossings, 0);
        assert_eq!(metrics.bends, 0);
        assert_eq!(metrics.label_gap, 0);
        assert_eq!(metrics.max_label_gap, 0);
        assert_eq!(metrics.extra_layers, 0);
        assert_eq!(metrics.back_edges, 0);

        let mut diamond = parser::parse("graph TD\n A{Decision} -->|yes| B\n A -->|no| C").unwrap();
        diamond.direction = direction;
        let metrics = measure(&diamond);
        assert_eq!(metrics.crossings, 0);
        assert_eq!(metrics.label_gap, 0);
        assert_eq!(metrics.max_label_gap, 0);

        let mut crossing =
            parser::parse("graph TD\n A --> C\n A --> D\n B --> C\n B --> D").unwrap();
        crossing.direction = direction;
        assert_eq!(measure(&crossing).crossings, 1, "{direction:?}");

        let mut long_edge = parser::parse("graph TD\n A --> B --> C --> D\n A --> D").unwrap();
        long_edge.direction = direction;
        assert_eq!(measure(&long_edge).extra_layers, 2);
    }

    let chain = parser::parse("graph TD\n A --> B --> C").unwrap();
    assert_eq!(measure(&chain).width, 5);
    assert_eq!(measure(&chain).height, 13);
    let crossing = parser::parse("graph TD\n A --> C\n A --> D\n B --> C\n B --> D").unwrap();
    assert_eq!(measure(&crossing).bends, 6);
    let distant_label =
        parser::parse("graph TD\n A{Decision} --> B --> C --> D --> E --> F\n A -->|far| F")
            .unwrap();
    let metrics = measure(&distant_label);
    assert_eq!(metrics.label_gap, 1);
    assert_eq!(metrics.max_label_gap, 1);
    let cycle = parser::parse("graph TD\n A --> B --> C --> A").unwrap();
    assert_eq!(measure(&cycle).back_edges, 1);
    let self_loop = parser::parse("graph TD\n A --> A").unwrap();
    let metrics = measure(&self_loop);
    assert_eq!(metrics.back_edges, 0);
    assert_eq!(metrics.extra_layers, 0);

    let reversed_label =
        parser::parse("graph TD\n A --> B --> C --> D --> E --> F{Decision}\n F -->|back| A")
            .unwrap();
    let metrics = measure(&reversed_label);
    assert_eq!(metrics.label_gap, 2);
    assert_eq!(metrics.max_label_gap, 2);
    assert_eq!(metrics.back_edges, 1);
    let unicode = parser::parse("graph TD\n A[界界]").unwrap();
    assert_eq!(measure(&unicode).width, 9);
    assert_eq!(measure(&unicode).height, 3);
    let empty = parser::parse("graph TD").unwrap();
    assert_eq!(measure(&empty), Metrics::default());
}

#[test]
fn metrics_are_unchanged_by_reflection() {
    for (path, mut diagram) in diagrams() {
        for (forward, backward) in [
            (Direction::TopDown, Direction::BottomUp),
            (Direction::LeftRight, Direction::RightLeft),
        ] {
            diagram.direction = forward;
            let expected = measure(&diagram);
            diagram.direction = backward;
            assert_eq!(measure(&diagram), expected, "{path} {backward:?}");
        }
    }
}
