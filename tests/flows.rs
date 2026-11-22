#[cfg_attr(not(feature = "cli"), allow(unused_imports))]
mod common;

use casemap::flows;
use common::TempRepo;
use std::fs;

const INVALID_UTF8: [u8; 2] = [0xff, 0xfe];

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

#[test]
fn section_rename_keeps_the_markdown_report_quiet() {
    let source = "# Pay\n\nPay for an order.\n\n```mermaid\nflowchart TD\nsubgraph payment [Payment]\na([Customer]) --> b[Pay] --> c[Done]\nend\n```\n";
    let before = vec![flows::parse_flow("pay.md", source)];
    let renamed = vec![flows::parse_flow(
        "pay.md",
        &source.replace("[Payment]", "[Payment details]"),
    )];
    let report = casemap::check::Report {
        flows: 1,
        problems: vec![],
    };
    let render = |after: &[flows::Flow]| {
        flows::render_markdown(&flows::MarkdownInput {
            changes: &flows::compare(&before, after),
            stale: &[],
            since: "HEAD",
            base: "abcdef123",
            report: &report,
            catalog: "docs/flows",
            uncovered: &[],
        })
    };
    assert!(
        flows::compare(&before, &renamed)
            .iter()
            .all(|change| !matches!(change, flows::FlowChange::Modified { .. }))
    );
    assert_eq!(render(&renamed), render(&before));
    assert!(render(&renamed).contains(flows::QUIET_MARKER));
}
