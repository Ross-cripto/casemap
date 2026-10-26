use casemap::{
    coverage::{self, Selection},
    flows,
};

#[test]
fn code_path_classification() {
    for (path, expected) in [
        ("app/migrations/0001_initial.py", false),
        ("next.config.js", false),
        ("tailwind.config.ts", false),
        ("vite.config.ts", false),
        ("src/types/api.d.ts", false),
        ("app/views.py", true),
        ("src/config.ts", true),
        ("src/a.rs", true),
        ("Tests/A.swift", false),
        ("Vendor/x.go", false),
        ("src/A.TS", true),
        ("web/App.tsx", true),
        ("api/x.py", true),
        ("README.md", false),
        ("docs/a.md", false),
        ("tests/a.rs", false),
        ("src/a_test.go", false),
        ("src/a.test.ts", false),
        ("src/a.spec.ts", false),
        ("src/a.test.d.ts", false),
        ("src/a_test.generated.go", false),
        ("test_a.py", false),
        ("a_spec.rb", false),
        ("vendor/x.go", false),
        ("node_modules/x.js", false),
        ("target/x.rs", false),
        ("dist/x.js", false),
        ("build/x.js", false),
        (".github/x.yml", false),
        ("Cargo.toml", false),
        ("docs/flows/own.rs", false),
        ("docs/flows-extra/a.rs", true),
        ("src/test_helpers.rs", true),
        ("src/test_a.rs", true),
    ] {
        assert_eq!(
            coverage::is_code_path(path, "docs/flows/", &Selection::default()),
            expected,
            "{path}"
        );
    }
    let selection = Selection {
        include: vec!["**".into()],
        exclude: vec!["vendor".into()],
    };
    assert!(coverage::is_code_path(
        "tests/a.rs",
        "docs/flows",
        &selection
    ));
    assert!(coverage::is_code_path(
        "README.md",
        "docs/flows",
        &selection
    ));
    assert!(!coverage::is_code_path(
        "vendor/a.rs",
        "docs/flows",
        &selection
    ));
    assert!(!coverage::is_code_path(
        "docs/flows/a.rs",
        "docs/flows",
        &selection
    ));
    assert!(coverage::is_code_path(
        "docs/flows/a.rs",
        "docs/fl?ws/",
        &Selection::default()
    ));
    assert!(!coverage::is_code_path(
        "docs/fl?ws/a.rs",
        "docs/fl?ws/",
        &Selection::default()
    ));
    let selection = Selection {
        include: vec!["web".into()],
        exclude: vec![],
    };
    assert!(!coverage::is_code_path(
        "src/a.rs",
        "docs/flows",
        &selection
    ));
}

#[test]
fn coverage_agrees_with_which() {
    let flows = vec![
        flows::parse_flow(
            "known.mmd",
            "graph TD\na[A]\n%% @code a src/known.rs\n%% @code absent src/unknown.rs\n%% @code * web/**",
        ),
        flows::parse_flow(
            "broken.mmd",
            "graph TD\na -->\n%% @code a src/broken.rs\n%% @code * src/whole.rs",
        ),
    ];
    for path in [
        "src/known.rs",
        "src/unknown.rs",
        "web/App.tsx",
        "src/broken.rs",
        "src/whole.rs",
        "api/x.py",
    ] {
        let files = vec![path.to_string()];
        let expected = !flows::anchored_flows(&flows, &files).is_empty();
        let report = coverage::repository(&files, &flows, "docs/flows/", &Selection::default());
        assert_eq!(report.covered == 1, expected, "{path}");
        assert_eq!(
            coverage::uncovered_changes(&files, &flows).is_empty(),
            expected,
            "{path}"
        );
    }
}

#[test]
fn repository_groups_and_sorts_files() {
    let files = ["src/z.rs", "web/b.ts", "src/a.rs", "root.py", "src/pay.rs"].map(String::from);
    let flows = vec![flows::parse_flow(
        "pay.mmd",
        "graph TD\na[A]\n%% @code a src/pay.rs",
    )];
    let report = coverage::repository(&files, &flows, "docs/flows/", &Selection::default());
    assert_eq!(
        (report.files, report.covered, report.anchored),
        (5, 1, true)
    );
    assert_eq!(
        report
            .directories
            .iter()
            .map(|directory| directory.path.as_str())
            .collect::<Vec<_>>(),
        ["", "src", "web"]
    );
    let source = &report.directories[1];
    assert_eq!((source.files, source.covered), (3, 1));
    assert_eq!(source.uncovered, ["a.rs", "z.rs"]);
    assert_eq!(
        coverage::uncovered_changes(&files, &flows),
        ["root.py", "src/a.rs", "src/z.rs", "web/b.ts"]
    );
}

#[test]
fn empty_repository_and_unknown_anchors() {
    let flows = vec![flows::parse_flow(
        "unknown.mmd",
        "graph TD\na[A]\n%% @code missing src/**",
    )];
    let report = coverage::repository(&[], &flows, "docs/flows/", &Selection::default());
    assert_eq!(
        (report.files, report.covered, report.anchored),
        (0, 0, false)
    );
    assert!(report.directories.is_empty());
}
