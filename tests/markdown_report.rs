use casemap::{
    anchors::Target,
    check::{Code, Problem, Report},
    flows::{
        self, COMMENT_LIMIT, COMMENT_MARKER, DIAGRAM_LIMIT, FlowChange, QUIET_MARKER, Stale,
        StaleHit,
    },
};

fn clean_report() -> Report {
    Report {
        flows: 1,
        problems: Vec::new(),
    }
}

#[test]
fn markdown_quiet_contains_only_markers_and_heading() {
    assert_eq!(
        flows::render_markdown(&[], &[], "HEAD", "abcdef123", &clean_report(), "docs/flows"),
        format!(
            "{COMMENT_MARKER}\n{QUIET_MARKER}\n### 🗺️ casemap · no use-case changes · check ok\n"
        )
    );
}

#[test]
fn markdown_draws_changed_new_and_removed_flows_in_order() {
    let before = vec![
        flows::parse_flow("pay.mmd", "graph TD\na[Pay] --> b[Done]"),
        flows::parse_flow("old.mmd", "graph TD\na[Old]"),
    ];
    let after = vec![
        flows::parse_flow("pay.mmd", "graph TD\na[Pay] --> c[Review] --> b[Done]"),
        flows::parse_flow("new.mmd", "graph TD\nx[New]"),
    ];
    let output = flows::render_markdown(
        &flows::compare(&before, &after),
        &[],
        "main",
        "abcdef123",
        &clean_report(),
        "docs/flows",
    );
    assert!(output.starts_with("<!-- casemap:pr-comment -->\n### 🗺️ casemap · 3 use cases changed · 0 possibly stale · check ok\nCompared with `main` (abcdef1).\n"));
    assert!(output.contains("#### + new · `docs/flows/new.mmd` (new)"));
    assert!(output.contains("#### − old · `docs/flows/old.mmd` (removed)"));
    assert!(output.contains("class n0 removed"));
    assert!(!output.contains("[\"+ "));
    assert!(!output.contains("|\"+\"|"));
    assert!(output.contains("#### ~ pay · `docs/flows/pay.mmd`\n+1 step"));
    assert!(output.contains("<details><summary>Changes</summary>\n\n- + step c"));
    assert!(output.find("#### +").unwrap() < output.find("#### −").unwrap());
    assert!(output.find("#### −").unwrap() < output.find("#### ~").unwrap());
    assert!(output.ends_with("#### Check\n\nok\n"));
}

#[test]
fn markdown_includes_broken_stale_and_check_problem_locations() {
    let flow = flows::parse_flow("pay.mmd", "graph TD\na -->");
    let changes = flows::compare(&[], std::slice::from_ref(&flow));
    let stale = vec![Stale {
        flow,
        hits: vec![
            StaleHit {
                target: Target::Flow,
                files: (0..8).map(|index| format!("src/file{index}.rs")).collect(),
            },
            StaleHit {
                target: Target::Step("pay".into()),
                files: vec!["src/pay.rs".into()],
            },
        ],
    }];
    let report = Report {
        flows: 1,
        problems: vec![
            Problem {
                file: Some("pay.mmd".into()),
                line: Some(2),
                code: Code::ParseError,
                message: "link has no target node".into(),
            },
            Problem {
                file: None,
                line: None,
                code: Code::EmptyCatalog,
                message: "no flows".into(),
            },
        ],
    };
    let output =
        flows::render_markdown(&changes, &stale, "HEAD", "abcdef123", &report, "docs/flows");
    assert!(output.contains("1 use case changed · 1 possibly stale · check 1 error, 1 warning"));
    assert!(output.contains(
        "#### ✗ pay · `docs/flows/pay.mmd` (parse error)\n\n`line 2: link has no target node`"
    ));
    assert!(output.contains("#### ⚠️ Possibly stale"));
    assert!(output.contains("… (+3 more)"));
    assert!(!output.contains("src/file5.rs"));
    assert!(output.contains("pay: src/pay.rs"));
    assert!(
        output.contains("- `docs/flows/pay.mmd:2` error: link has no target node [parse-error]")
    );
    assert!(output.contains("- `docs/flows` warning: no flows [empty-catalog]"));
}

#[test]
fn markdown_preserves_first_diagrams_when_capping() {
    let mut changes = Vec::new();
    for index in 0..80 {
        let source = format!("graph TD\na[\"{}\"] --> b[Done]", "🗺".repeat(400));
        let flow = flows::parse_flow(&format!("flow-{index}.mmd"), &source);
        changes.extend(flows::compare(&[], &[flow]));
    }
    let flow = flows::parse_flow("stale.mmd", "graph TD\na[Stale]");
    let stale = vec![Stale {
        flow,
        hits: vec![StaleHit {
            target: Target::Flow,
            files: vec!["src/pay.rs".into()],
        }],
    }];
    let report = Report {
        flows: 1,
        problems: vec![Problem {
            file: None,
            line: None,
            code: Code::EmptyCatalog,
            message: "Check retained".into(),
        }],
    };
    let output =
        flows::render_markdown(&changes, &stale, "main", "abcdef123", &report, "docs/flows");
    assert!(
        output.chars().count() <= COMMENT_LIMIT,
        "{}",
        output.chars().count()
    );
    assert!(output.starts_with(COMMENT_MARKER));
    assert!(output.contains("#### ⚠️ Possibly stale"));
    assert!(output.contains("Check retained"));
    assert!(output.contains("#### Check"));
    assert!(output.contains(
        "Diagram omitted to fit GitHub's comment limit; run `casemap diff main` locally."
    ));
    assert!(output.contains("#### + flow-79 · `docs/flows/flow-79.mmd` (new)\n"));
    let sections: Vec<_> = output.split("\n#### + ").skip(1).collect();
    assert!(sections.first().unwrap().contains("```mermaid"));
    assert!(!sections.last().unwrap().contains("```mermaid"));
    let mut omitted = false;
    for section in sections {
        if section.contains("Diagram omitted") {
            omitted = true;
        } else if section.contains("```mermaid") {
            assert!(!omitted, "a later diagram survived an earlier omission");
        }
    }
    assert_eq!(
        output.matches("```mermaid").count() * 2,
        output.matches("```").count()
    );
    assert_eq!(
        output.matches("<details>").count(),
        output.matches("</details>").count()
    );
    assert!(!output.contains(
        "#### + flow-79 · `docs/flows/flow-79.mmd` (new)\n+2 steps, +1 link\n\n```mermaid"
    ));
}

#[test]
fn markdown_omits_details_after_all_diagrams_when_needed() {
    let flow = flows::parse_flow(
        "large.mmd",
        &format!("graph TD\na[\"{}\"]", "x".repeat(COMMENT_LIMIT)),
    );
    let changes: Vec<FlowChange> = flows::compare(&[], &[flow]);
    let output = flows::render_markdown(
        &changes,
        &[],
        "HEAD",
        "abcdef1",
        &clean_report(),
        "docs/flows",
    );
    assert!(output.chars().count() <= COMMENT_LIMIT);
    assert!(!output.contains("```mermaid"));
    assert!(!output.contains("<details>"));
    assert!(output.ends_with("#### Check\n\nok\n"));
}

#[test]
fn markdown_is_capped_for_github() {
    markdown_preserves_first_diagrams_when_capping();
    let flow = flows::parse_flow("stale.mmd", "graph TD\na[Stale]");
    let stale: Vec<_> = (0..10_000)
        .map(|_| Stale {
            flow: flow.clone(),
            hits: vec![StaleHit {
                target: Target::Flow,
                files: vec!["src/pay.rs".into()],
            }],
        })
        .collect();
    let report = Report {
        flows: 10_000,
        problems: (0..10_000)
            .map(|_| Problem {
                file: None,
                line: None,
                code: Code::EmptyCatalog,
                message: "A problem".into(),
            })
            .collect(),
    };
    let output = flows::render_markdown(&[], &stale, "main", "abcdef123", &report, "docs/flows");
    assert!(output.chars().count() <= COMMENT_LIMIT);
    assert!(output.contains("more problems; run `casemap check` locally."));
    assert!(output.contains("more stale flows; run `casemap diff main` locally."));
    assert!(output.starts_with(COMMENT_MARKER));

    for label in ["x".repeat(DIAGRAM_LIMIT), "🗺".repeat(DIAGRAM_LIMIT / 2)] {
        let huge = flows::parse_flow("huge.mmd", &format!("graph TD\na[\"{label}\"]"));
        let output = flows::render_markdown(
            &flows::compare(&[], &[huge]),
            &[],
            "main",
            "abcdef123",
            &clean_report(),
            "docs/flows",
        );
        assert!(output.chars().count() <= COMMENT_LIMIT);
        assert!(output.contains("Diagram too large for GitHub"));
        for block in output.split("\n```mermaid\n").skip(1) {
            assert!(block.split("\n```").next().unwrap().encode_utf16().count() <= DIAGRAM_LIMIT);
        }
    }
}

#[test]
fn markdown_escapes_untrusted_text_and_uses_safe_inline_fences() {
    let mut flow = flows::parse_flow("x.mmd", "graph TD\na[\"A ``` </details> #35;quot;\"]");
    flow.path = "x`y.md".into();
    flow.title = "~title~ & *title* _x_ [link] <details> # | @user \\ `".into();
    let report = Report {
        flows: 1,
        problems: vec![Problem {
            file: Some("x`y.md".into()),
            line: Some(2),
            code: Code::ParseError,
            message: "@someone <details> *text* `code`".into(),
        }],
    };
    let output = flows::render_markdown(
        &flows::compare(&[], &[flow]),
        &[],
        "main`branch",
        "abcdef123",
        &report,
        "docs/flows",
    );
    assert!(output.contains(r"\~title\~ \& \*title\* \_x\_ \[link\] \<details\> \# \| \@user"));
    assert!(output.contains("``docs/flows/x`y.md``"));
    assert!(output.contains("Compared with ``main`branch`` (abcdef1)."));
    assert!(output.contains(r"\@someone \<details\> \*text\* \`code\`"));
    assert!(output.contains("#96;#96;#96; #lt;/details#gt; #35;quot;"));
    assert_eq!(output.matches("\n</details>").count(), 1);
}

#[test]
fn markdown_does_not_repeat_a_sha_prefix() {
    let report = Report {
        flows: 0,
        problems: vec![Problem {
            file: None,
            line: None,
            code: Code::EmptyCatalog,
            message: "No flows".into(),
        }],
    };
    let output = flows::render_markdown(&[], &[], "abcdef1", "abcdef123", &report, "docs/flows");
    assert!(output.contains("Compared with abcdef1.\n"));
    assert!(!output.contains("(abcdef1)"));
}

#[test]
fn markdown_caps_oversized_flow_headings() {
    let mut flow = flows::parse_flow("huge.mmd", "graph TD\na[A]");
    flow.title = "x".repeat(COMMENT_LIMIT * 2);
    let output = flows::render_markdown(
        &flows::compare(&[], &[flow]),
        &[],
        "main",
        "abcdef123",
        &clean_report(),
        "docs/flows",
    );
    assert!(output.chars().count() <= COMMENT_LIMIT);
    assert!(output.starts_with(COMMENT_MARKER));
    assert!(output.contains("1 more changed flows"));
    assert!(output.contains("#### Check\n\nok\n"));
}

#[test]
fn cap_drops_flow_sections_before_check_and_stale() {
    let mut changes = Vec::new();
    for index in 0..400 {
        let mut flow = flows::parse_flow(&format!("flow-{index}.mmd"), "graph TD\na[A]");
        flow.title = format!("Flow {index} {}", "x".repeat(150));
        changes.extend(flows::compare(&[], &[flow]));
    }
    let stale: Vec<_> = (0..2)
        .map(|index| Stale {
            flow: flows::parse_flow(&format!("stale-{index}.mmd"), "graph TD\na[Stale]"),
            hits: vec![StaleHit {
                target: Target::Flow,
                files: vec![format!("src/stale-{index}.rs")],
            }],
        })
        .collect();
    let report = Report {
        flows: 400,
        problems: (0..3)
            .map(|index| Problem {
                file: None,
                line: None,
                code: Code::EmptyCatalog,
                message: format!("Check problem {index}"),
            })
            .collect(),
    };
    let output =
        flows::render_markdown(&changes, &stale, "main", "abcdef123", &report, "docs/flows");
    assert!(output.chars().count() <= COMMENT_LIMIT);
    assert!(output.contains("more changed flows"));
    assert!(output.contains("#### + Flow 0 "));
    assert!(!output.contains("#### + Flow 399 "));
    for index in 0..3 {
        assert!(output.contains(&format!("Check problem {index}")));
    }
    for index in 0..2 {
        assert!(output.contains(&format!("docs/flows/stale-{index}.mmd")));
        assert!(output.contains(&format!("src/stale-{index}.rs")));
    }
}

#[test]
fn diagrams_over_the_edge_limit_are_omitted() {
    for (edges, omitted) in [(500, false), (501, true)] {
        let mut source = String::from("graph TD\n");
        for index in 0..edges {
            source.push_str(&format!("a --> n{index}\n"));
        }
        let flow = flows::parse_flow("edges.mmd", &source);
        let output = flows::render_markdown(
            &flows::compare(&[], &[flow]),
            &[],
            "main",
            "abcdef123",
            &clean_report(),
            "docs/flows",
        );
        assert_eq!(
            output.contains("Diagram too large for GitHub; run `casemap diff main` locally."),
            omitted
        );
        assert_eq!(output.contains("```mermaid"), !omitted);
    }
}
