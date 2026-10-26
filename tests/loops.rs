use casemap::layout::{self, ItemKind};
use casemap::{Direction, Options, parser, quality};

#[test]
fn diff_unions_keep_loop_edge_colours() {
    let forward = parser::parse("graph TD\n A --> B --> C --> D").unwrap();
    let with_loop = parser::parse("graph TD\n A --> B --> C --> D\n D -->|again| B").unwrap();
    let changed_loop = parser::parse("graph TD\n A --> B --> C --> D\n D -->|retry| B").unwrap();
    for (before, after, mark, colour) in [
        (
            &forward,
            &with_loop,
            casemap::model::Mark::Added,
            "\x1b[32m",
        ),
        (
            &with_loop,
            &forward,
            casemap::model::Mark::Removed,
            "\x1b[31m",
        ),
        (
            &with_loop,
            &changed_loop,
            casemap::model::Mark::Changed,
            "\x1b[33m",
        ),
    ] {
        let difference = casemap::diff::diff(before, after);
        let diagram = &difference.union;
        let layout = layout::layout(diagram);
        let routing = casemap::route::route(diagram, &layout);
        let loop_edge = diagram
            .edges
            .iter()
            .position(|edge| edge.from == 3 && edge.to == 1)
            .unwrap();
        assert_eq!(diagram.edges[loop_edge].mark, mark);
        let loop_groups: Vec<_> = routing
            .groups
            .iter()
            .filter(|group| {
                layout.out_segs[group.src]
                    .iter()
                    .any(|&segment| layout.segs[segment].edge == loop_edge)
            })
            .collect();
        assert!(loop_groups.iter().all(|group| group.mark == mark));
        assert_eq!(
            loop_groups
                .iter()
                .flat_map(|group| &group.dests)
                .filter(|dest| dest.arrow)
                .count(),
            1
        );
        let output = casemap::render_diagram(
            diagram,
            &Options {
                color: true,
                ..Options::default()
            },
        );
        assert!(output.contains(&format!("{colour}▼")), "{mark:?}\n{output}");
        assert!(!output.contains('▲'), "{mark:?}\n{output}");
    }
}

#[test]
fn nested_returns_do_not_cross() {
    let diagram = parser::parse("graph TD\n A --> B --> C --> D\n C --> B\n D --> B").unwrap();
    assert_eq!(quality::measure(&diagram).crossings, 0);
}

#[test]
fn loops_enter_their_target_from_above() {
    for (direction, forward_arrow, upstream_arrow, ascii_forward, ascii_upstream) in [
        (Direction::TopDown, '▼', '▲', 'v', '^'),
        (Direction::BottomUp, '▲', '▼', '^', 'v'),
        (Direction::LeftRight, '▶', '◀', '>', '<'),
        (Direction::RightLeft, '◀', '▶', '<', '>'),
    ] {
        let mut diagram = parser::parse("graph TD\n A --> B --> C --> D\n D -->|again| B").unwrap();
        diagram.direction = direction;
        let layout = layout::layout(&diagram);
        let loop_segments: Vec<_> = layout
            .segs
            .iter()
            .filter(|segment| segment.edge == 3)
            .collect();
        let target_arrows: Vec<_> = loop_segments
            .iter()
            .filter(|segment| layout.arrow_at_dst(&diagram, segment))
            .collect();
        assert_eq!(target_arrows.len(), 1, "{direction:?}");
        let target_segment = target_arrows[0];
        assert_eq!(layout.items[target_segment.to].kind, ItemKind::Node(1));
        assert_eq!(
            layout.items[target_segment.from].rank + 1,
            layout.items[target_segment.to].rank
        );
        assert!(
            loop_segments
                .iter()
                .all(|segment| !layout.arrow_at_src(&diagram, segment))
        );
        let source_item = layout
            .items
            .iter()
            .find(|item| item.kind == ItemKind::Node(3))
            .unwrap();
        let label_item = layout
            .items
            .iter()
            .find(|item| item.kind == ItemKind::Label("again".into()))
            .unwrap();
        assert_eq!(label_item.rank + 1, source_item.rank);

        for (ascii, forward, upstream) in [
            (false, forward_arrow, upstream_arrow),
            (true, ascii_forward, ascii_upstream),
        ] {
            let output = casemap::render_diagram(
                &diagram,
                &Options {
                    ascii,
                    ..Options::default()
                },
            );
            assert_eq!(
                output.chars().filter(|&glyph| glyph == forward).count(),
                4,
                "{direction:?}\n{output}"
            );
            assert!(!output.contains(upstream), "{direction:?}\n{output}");
            assert!(output.contains("again"), "{direction:?}\n{output}");
        }
        assert_eq!(quality::measure(&diagram).upstream_arrows, 0);
    }
}
