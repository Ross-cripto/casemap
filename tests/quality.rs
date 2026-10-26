use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use casemap::layout::{self, ItemKind};
use casemap::quality::{Metrics, measure};
use casemap::{Diagram, Direction, flows, parser};

const MINIMUM_DIAGRAMS: usize = 29 + 10 + 1;

const WIDTH_AND_CROSSING_BUDGETS: &[(&str, usize, usize)] = &[
    ("docs/flows/browse-the-catalog.md", 112, 1),
    ("docs/flows/check-the-catalog.md", 76, 0),
    ("docs/flows/keep-the-map-true.md", 68, 0),
    ("docs/flows/map-a-new-repository.md", 93, 0),
    ("docs/flows/measure-coverage.md", 99, 0),
    ("docs/flows/render-a-diagram.md", 66, 0),
    ("docs/flows/review-a-pull-request.md", 107, 0),
    ("docs/flows/review-flow-changes.md", 70, 0),
    ("docs/flows/set-up-a-catalog.md", 76, 1),
    ("docs/flows/watch-a-diagram.md", 64, 0),
    ("examples/architecture.mmd", 132, 1),
    ("examples/simple.mmd", 44, 0),
    (
        "tests/fixtures/corpus/commerce/add-an-item-to-the-cart.md",
        107,
        0,
    ),
    (
        "tests/fixtures/corpus/commerce/browse-a-collection.md",
        81,
        0,
    ),
    (
        "tests/fixtures/corpus/commerce/browse-the-storefront.md",
        48,
        0,
    ),
    ("tests/fixtures/corpus/commerce/check-out-a-cart.md", 64, 0),
    ("tests/fixtures/corpus/commerce/manage-the-cart.md", 66, 0),
    ("tests/fixtures/corpus/commerce/read-a-store-page.md", 59, 0),
    (
        "tests/fixtures/corpus/commerce/refresh-the-catalogue-after-a-store-edit.md",
        76,
        0,
    ),
    (
        "tests/fixtures/corpus/commerce/search-for-products.md",
        121,
        0,
    ),
    ("tests/fixtures/corpus/commerce/view-a-product.md", 87, 0),
    ("tests/fixtures/corpus/healthchecks/add-a-check.md", 100, 0),
    (
        "tests/fixtures/corpus/healthchecks/change-a-check-schedule.md",
        119,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/create-a-check-by-ping-url.md",
        156,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/get-alerted-when-a-check-goes-down.md",
        64,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/manage-checks-through-the-api.md",
        91,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/pause-and-resume-a-check.md",
        138,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/send-a-ping-from-a-job.md",
        97,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/sign-in-with-an-email-link.md",
        104,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/sign-up-for-an-account.md",
        64,
        0,
    ),
    (
        "tests/fixtures/corpus/healthchecks/test-a-notification-channel.md",
        81,
        0,
    ),
    (
        "tests/fixtures/corpus/shiori/add-a-bookmark-from-the-terminal.md",
        180,
        0,
    ),
    ("tests/fixtures/corpus/shiori/add-a-bookmark.md", 87, 0),
    (
        "tests/fixtures/corpus/shiori/find-and-open-bookmarks-in-the-terminal.md",
        132,
        0,
    ),
    (
        "tests/fixtures/corpus/shiori/import-and-export-bookmarks.md",
        137,
        0,
    ),
    (
        "tests/fixtures/corpus/shiori/manage-my-bookmarks.md",
        146,
        1,
    ),
    ("tests/fixtures/corpus/shiori/manage-tags.md", 101, 0),
    (
        "tests/fixtures/corpus/shiori/manage-user-accounts.md",
        80,
        0,
    ),
    ("tests/fixtures/corpus/shiori/read-a-saved-page.md", 106, 0),
    (
        "tests/fixtures/corpus/shiori/sign-in-to-the-web-ui.md",
        56,
        0,
    ),
    (
        "tests/fixtures/corpus/shiori/update-or-delete-bookmarks-in-the-terminal.md",
        117,
        0,
    ),
];

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
            // Snapshot rows and budgets use '/' so Windows reports the same paths.
            let name = path
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string()
                .replace('\\', "/");
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
        "{:<path_width$}  {:>5} {:>6} {:>9} {:>5} {:>9} {:>13} {:>12} {:>10} {:>15}\n",
        "path",
        "width",
        "height",
        "crossings",
        "bends",
        "label_gap",
        "max_label_gap",
        "extra_layers",
        "back_edges",
        "upstream_arrows"
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
        totals.upstream_arrows += metrics.upstream_arrows;
    }
    write_row(&mut output, "TOTAL", &totals, path_width);
    output
}

fn write_row(output: &mut String, path: &str, metrics: &Metrics, path_width: usize) {
    writeln!(
        output,
        "{path:<path_width$}  {:>5} {:>6} {:>9} {:>5} {:>9} {:>13} {:>12} {:>10} {:>15}",
        metrics.width,
        metrics.height,
        metrics.crossings,
        metrics.bends,
        metrics.label_gap,
        metrics.max_label_gap,
        metrics.extra_layers,
        metrics.back_edges,
        metrics.upstream_arrows
    )
    .unwrap();
}

fn node_center(layout: &layout::Layout, diagram: &Diagram, id: &str) -> i32 {
    let node = diagram.nodes.iter().position(|node| node.id == id).unwrap();
    layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Node(node))
        .unwrap()
        .center()
}

fn assert_fanout_spine(source: &str, continuation: &str, side_branches: [&str; 2]) {
    for direction in [
        Direction::TopDown,
        Direction::BottomUp,
        Direction::LeftRight,
        Direction::RightLeft,
    ] {
        let mut diagram = parser::parse(source).unwrap();
        diagram.direction = direction;
        let layout = layout::layout(&diagram);
        let center = |id: &str| node_center(&layout, &diagram, id);
        let spine_center = center(continuation);
        assert_eq!(center("decision"), spine_center, "{direction:?}");
        let side_centers = [center(side_branches[0]), center(side_branches[1])];
        assert!(
            side_centers.iter().any(|&value| value < spine_center)
                && side_centers.iter().any(|&value| value > spine_center),
            "{direction:?}: branches {side_centers:?}, spine {spine_center}"
        );
    }
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
fn chained_decisions_keep_a_straight_spine() {
    let source = "flowchart TD
        first{First decision?} -->|no| first_error[First error]
        first -->|yes| second{Second decision?}
        second -->|no| second_error[Second error]
        second -->|yes| third{Third decision?}
        third -->|no| third_error[Third error]
        third -->|yes| fourth{Fourth decision?}
        fourth -->|no| fourth_error[Fourth error]
        fourth -->|yes| done[Done]
        done --> finish[Finished]";
    for direction in [
        Direction::TopDown,
        Direction::BottomUp,
        Direction::LeftRight,
        Direction::RightLeft,
    ] {
        let mut diagram = parser::parse(source).unwrap();
        diagram.direction = direction;
        let layout = layout::layout(&diagram);
        let mut centers = Vec::new();
        for id in ["first", "second", "third", "fourth", "done", "finish"] {
            centers.push(node_center(&layout, &diagram, id));
        }
        let spread = centers.iter().max().unwrap() - centers.iter().min().unwrap();
        assert!(spread <= 1, "{direction:?}: spine centers {centers:?}");
        let widest_box = layout
            .items
            .iter()
            .filter(|item| item.is_node())
            .map(|item| item.ext_o)
            .max()
            .unwrap();
        let spacing = if direction.is_vertical() { 4 } else { 1 };
        let metrics = measure(&diagram);
        let order_extent = if direction.is_vertical() {
            metrics.width
        } else {
            metrics.height
        };
        assert!(
            order_extent <= (2 * widest_box + spacing) as usize,
            "{direction:?}: extent {order_extent}, widest box {widest_box}"
        );
    }
}

#[test]
fn fanout_places_short_branches_on_both_sides_of_the_spine() {
    let source = "flowchart TD
        decision{Choose a path} -->|first| first[First error]
        decision -->|second| second[Second error]
        decision -->|continue| next[Continue] --> finish[Finish]";
    assert_fanout_spine(source, "next", ["first", "second"]);
}

#[test]
fn broad_fanout_continuation_stays_on_the_spine() {
    let source = "flowchart TD
        decision{Choose a path} -->|short| short[Short branch]
        decision -->|long| long[Long branch] --> middle[Middle] --> later[Later] --> end[End]
        decision -->|broad| broad[Several outcomes]
        broad --> first[First] & second[Second] & third[Third] & fourth[Fourth]";
    assert_fanout_spine(source, "broad", ["short", "long"]);
}

#[test]
fn equal_continuations_use_node_index_before_edge_order() {
    let source = "flowchart TD
        decision{Choose}
        preferred[Preferred path]
        other[Alternate path]
        decision -->|other| other
        decision -->|preferred| preferred
        preferred --> preferred_end[Preferred end]
        other --> other_end[Alternate end]";
    for direction in [
        Direction::TopDown,
        Direction::BottomUp,
        Direction::LeftRight,
        Direction::RightLeft,
    ] {
        let mut diagram = parser::parse(source).unwrap();
        diagram.direction = direction;
        let layout = layout::layout(&diagram);
        assert_eq!(
            node_center(&layout, &diagram, "decision"),
            node_center(&layout, &diagram, "preferred"),
            "{direction:?}"
        );
        assert_ne!(
            node_center(&layout, &diagram, "decision"),
            node_center(&layout, &diagram, "other"),
            "{direction:?}"
        );
    }
}

#[test]
fn clusters_keep_members_inside_straight_borders() {
    let mut paths = Vec::new();
    collect_files(Path::new("examples"), "mmd", &mut paths);
    collect_files(Path::new("tests/fixtures"), "mmd", &mut paths);
    paths.sort();
    for path in paths {
        let mut diagram = parser::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        diagram.fold_self_loops();
        for direction in [
            Direction::TopDown,
            Direction::BottomUp,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            diagram.direction = direction;
            let layout = layout::layout(&diagram);
            for cluster in 0..diagram.clusters.len() {
                let mut border_positions = None;
                for layer in &layout.layers {
                    let left = layer.iter().position(|&item| {
                        layout.items[item].kind
                            == ItemKind::Border {
                                cluster,
                                left: true,
                            }
                    });
                    let right = layer.iter().position(|&item| {
                        layout.items[item].kind
                            == ItemKind::Border {
                                cluster,
                                left: false,
                            }
                    });
                    if left.is_none() && right.is_none() {
                        assert!(
                            layer
                                .iter()
                                .all(|&item| !layout.items[item].path.contains(&cluster))
                        );
                        continue;
                    }
                    let left = left.expect("cluster has a left border");
                    let right = right.expect("cluster has a right border");
                    assert!(left < right, "{path:?} {direction:?}");
                    let positions = (
                        layout.items[layer[left]].pos,
                        layout.items[layer[right]].pos,
                    );
                    if let Some(expected) = border_positions {
                        assert_eq!(positions, expected, "{path:?} {direction:?}");
                    }
                    border_positions = Some(positions);
                    for (slot, &item_index) in layer.iter().enumerate() {
                        let item = &layout.items[item_index];
                        assert_eq!(
                            item.path.contains(&cluster),
                            (left..=right).contains(&slot),
                            "{path:?} {direction:?}: cluster {cluster} is contiguous"
                        );
                        if left < slot && slot < right {
                            assert!(item.pos > positions.0, "{path:?} {direction:?}");
                            assert!(
                                item.pos + item.ext_o <= positions.1,
                                "{path:?} {direction:?}"
                            );
                        }
                    }
                }
            }
        }
    }
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
        assert_eq!(metrics.upstream_arrows, 0);
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
    assert_eq!(metrics.label_gap, 0);
    assert_eq!(metrics.max_label_gap, 0);
    let cycle = parser::parse("graph TD\n A --> B --> C --> A").unwrap();
    assert_eq!(measure(&cycle).back_edges, 1);
    assert_eq!(measure(&cycle).upstream_arrows, 0);
    let self_loop = parser::parse("graph TD\n A --> A").unwrap();
    let metrics = measure(&self_loop);
    assert_eq!(metrics.back_edges, 0);
    assert_eq!(metrics.extra_layers, 0);

    let reversed_label =
        parser::parse("graph TD\n A --> B --> C --> D --> E --> F{Decision}\n F -->|back| A")
            .unwrap();
    let metrics = measure(&reversed_label);
    assert_eq!(metrics.label_gap, 0);
    assert_eq!(metrics.max_label_gap, 0);
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

#[test]
fn labels_sit_next_to_their_source() {
    for source in [
        "graph TD\n A{Decision} --> B --> C --> D --> E --> F\n A -->|forward| F",
        "graph TD\n A --> B --> C --> D --> E --> F{Decision}\n F -->|back| A",
    ] {
        for direction in [
            Direction::TopDown,
            Direction::BottomUp,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            let mut diagram = parser::parse(source).unwrap();
            diagram.direction = direction;
            assert_eq!(assert_labels_next_to_source(&diagram), 1);
            assert_eq!(measure(&diagram).label_gap, 0);
            assert_eq!(measure(&diagram).max_label_gap, 0);
        }
    }
}

fn assert_labels_next_to_source(diagram: &Diagram) -> usize {
    let layout = layout::layout(diagram);
    let mut labels = 0;
    for (item_index, item) in layout.items.iter().enumerate() {
        if !matches!(item.kind, ItemKind::Label(_)) {
            continue;
        }
        labels += 1;
        let incoming = layout.in_segs[item_index][0];
        let edge_index = layout.segs[incoming].edge;
        let original_source = diagram.edges[edge_index].from;
        let source_item = layout
            .items
            .iter()
            .find(|candidate| candidate.kind == ItemKind::Node(original_source))
            .unwrap();
        let expected_rank = if layout.reversed[edge_index] {
            source_item.rank - 1
        } else {
            source_item.rank + 1
        };
        assert_eq!(item.rank, expected_rank, "edge {edge_index}: {diagram:?}");
    }
    labels
}

#[test]
fn corpus_stays_within_quality_budgets() {
    const MAXIMUM_CROSSINGS: usize = 4;
    const MAXIMUM_WIDTH: usize = 3180;
    const MAXIMUM_DIAGRAM_WIDTH: usize = 140;
    const MAXIMUM_TERMINAL_BOOKMARK_WIDTH: usize = 120;
    const MAXIMUM_BENDS: usize = 704;
    const MAXIMUM_EXTRA_LAYERS: usize = 457;
    const MAXIMUM_WIDTH_PERCENT: usize = 115;

    let mut total = Metrics::default();
    for (path, diagram) in diagrams() {
        let mut prepared = diagram.clone();
        prepared.fold_self_loops();
        assert_labels_next_to_source(&prepared);
        let metrics = measure(&diagram);
        assert!(
            metrics.width <= MAXIMUM_DIAGRAM_WIDTH,
            "{path}: {metrics:?}"
        );
        if path == "tests/fixtures/corpus/shiori/add-a-bookmark-from-the-terminal.md" {
            assert!(
                metrics.width <= MAXIMUM_TERMINAL_BOOKMARK_WIDTH,
                "{metrics:?}"
            );
        }
        let (_, baseline_width, baseline_crossings) = WIDTH_AND_CROSSING_BUDGETS
            .iter()
            .find(|(baseline_path, _, _)| *baseline_path == path)
            .unwrap_or_else(|| panic!("missing step-one budget for {path}"));
        assert!(
            metrics.width * 100 <= baseline_width * MAXIMUM_WIDTH_PERCENT,
            "{path}: width {} from {baseline_width}",
            metrics.width
        );
        assert!(
            metrics.crossings <= *baseline_crossings,
            "{path}: crossings {} from {baseline_crossings}",
            metrics.crossings
        );
        assert_eq!(metrics.upstream_arrows, 0, "{path}");
        assert_eq!(metrics.label_gap, 0, "{path}");
        assert_eq!(metrics.max_label_gap, 0, "{path}");
        total.crossings += metrics.crossings;
        total.width += metrics.width;
        total.bends += metrics.bends;
        total.extra_layers += metrics.extra_layers;
    }
    assert!(total.crossings <= MAXIMUM_CROSSINGS, "{total:?}");
    assert!(total.width <= MAXIMUM_WIDTH, "{total:?}");
    assert!(total.bends <= MAXIMUM_BENDS, "{total:?}");
    assert!(total.extra_layers <= MAXIMUM_EXTRA_LAYERS, "{total:?}");
}
