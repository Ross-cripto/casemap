use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const FLOWS_DIR: &str = "docs/flows";
const BASE_BRANCH: &str = "main";
const FEATURE_BRANCH: &str = "feature";
const LOGOUT_STEP: &str = "    home --> logout[Sign out]\n";
const INVALID_UTF8: [u8; 2] = [0xff, 0xfe];
const GIT_LOCATION_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

use strata::{diff, flows, Direction, Options};

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
            let output = strata::render_diagram(union, &opts);
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
            assert_eq!(output, strata::render_diagram(union, &opts));
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
            let ascii = strata::render_diagram(
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
    let output = strata::render_diagram(
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
                let output = strata::render_str(
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
    let output = strata::render_markdown(text, &Options::default()).unwrap();
    for expected in ["A\n", "B\n", "First", "Second", "Third", "Fourth"] {
        assert!(output.contains(expected), "missing {expected}\n{output}");
    }
    assert!(!output.contains("not a heading"));
    let error = strata::render_markdown(
        &text.replace("c[Third] --> d[Fourth]", "c -->"),
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(error.line, 13);
    assert_eq!(
        strata::render_markdown("# Empty", &Options::default()).unwrap(),
        ""
    );
}

fn isolate_git(command: &mut Command) {
    for variable in GIT_LOCATION_VARS {
        command.env_remove(variable);
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

fn git_command() -> Command {
    let mut command = Command::new("git");
    isolate_git(&mut command);
    command
        .env("GIT_AUTHOR_NAME", "strata")
        .env("GIT_AUTHOR_EMAIL", "strata@example.com")
        .env("GIT_COMMITTER_NAME", "strata")
        .env("GIT_COMMITTER_EMAIL", "strata@example.com")
        .args(["-c", "commit.gpgsign=false"]);
    command
}

fn strata_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_strata"));
    isolate_git(&mut command);
    command
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct TempRepo {
    directory: TempDir,
}

impl TempRepo {
    fn new() -> Self {
        let directory = TempDir::new("strata");
        let repo = Self { directory };
        fs::create_dir_all(repo.file(FLOWS_DIR)).unwrap();
        repo
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn file(&self, relative: &str) -> PathBuf {
        self.path().join(relative)
    }

    fn flow_path(&self, name: &str) -> PathBuf {
        self.file(FLOWS_DIR).join(name)
    }

    fn git(&self, args: &[&str]) {
        self.git_stdout(args);
    }

    fn git_stdout(&self, args: &[&str]) -> String {
        let output = git_command()
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn add_fixture(&self, side: &str, name: &str) {
        fs::copy(
            Path::new("tests/fixtures/flows").join(side).join(name),
            self.flow_path(name),
        )
        .unwrap();
    }

    fn copy_catalog(&self, side: &str) {
        for entry in fs::read_dir(format!("tests/fixtures/flows/{side}")).unwrap() {
            let name = entry.unwrap().file_name();
            self.add_fixture(side, name.to_str().unwrap());
        }
    }

    fn committed_before() -> Self {
        let repo = Self::new();
        repo.copy_catalog("before");
        repo.git(&["init", "-q", "-b", BASE_BRANCH]);
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", "base flows"]);
        repo
    }

    fn strata(&self, args: &[&str]) -> Output {
        strata_command()
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap()
    }
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn append_step(markdown: &str, step: &str) -> String {
    let closing_fence = markdown.rfind("```").expect("flow has a closing fence");
    let mut updated = markdown.to_string();
    updated.insert_str(closing_fence, step);
    updated
}

fn divergent_catalog() -> TempRepo {
    let repo = TempRepo::committed_before();
    repo.git(&["checkout", "-q", "-b", FEATURE_BRANCH]);
    fs::remove_file(repo.flow_path("legacy-export.md")).unwrap();
    repo.add_fixture("after", "refund.md");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "replace export with refund"]);
    repo.git(&["checkout", "-q", BASE_BRANCH]);
    let login = fs::read_to_string(repo.flow_path("login.md")).unwrap();
    fs::write(repo.flow_path("login.md"), append_step(&login, LOGOUT_STEP)).unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "add logout on main"]);
    repo.git(&["checkout", "-q", FEATURE_BRANCH]);
    repo.add_fixture("after", "checkout.md");
    repo
}

#[test]
fn cli_diff_uses_merge_base_not_ref() {
    let repo = divergent_catalog();
    let sha = repo.git_stdout(&["merge-base", BASE_BRANCH, "HEAD"]);
    let output = stdout(&repo.strata(&["diff", BASE_BRANCH, "--no-color"]));
    assert_eq!(
        output.lines().next().unwrap(),
        format!(
            "flow changes since main ({}): 1 modified, 1 new, 1 removed",
            &sha[..flows::SHORT_SHA_LEN]
        )
    );
}

#[test]
fn cli_diff_reports_base_that_did_not_parse() {
    let repo = TempRepo::new();
    let path = repo.flow_path("x.md");
    fs::write(&path, "# X\n```mermaid\ngraph TD\n a -->\n```\n").unwrap();
    repo.git(&["init", "-q", "-b", BASE_BRANCH]);
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "broken base"]);
    fs::write(path, "# X\n```mermaid\ngraph TD\na[A]\n```\n").unwrap();
    let output = stdout(&repo.strata(&["diff", "--no-color"]));
    assert!(
        output.contains("X (x.md)  new flow: +1 step (base version did not parse)"),
        "{output}"
    );
}

#[test]
fn cli_diff_against_branch_includes_uncommitted() {
    let repo = divergent_catalog();
    let output = stdout(&repo.strata(&["diff", BASE_BRANCH, "--no-color"]));
    for expected in [
        "1 modified, 1 new, 1 removed",
        "Checkout (checkout.md)  modified: +1 step, -1 step, ~1 step, +3 links, -3 links, ~1 link",
        "  ~ link pay -> error \"no\" -> \"declined\"",
        "Refund (refund.md)  new flow: +4 steps, +3 links",
        "Legacy export (legacy-export.md)  removed",
        "unchanged: login",
    ] {
        assert!(output.contains(expected), "missing {expected}\n{output}");
    }
    let output = stdout(&repo.strata(&["diff", "--no-color"]));
    assert!(output.contains("1 modified, 0 new, 0 removed"), "{output}");
}

#[test]
fn cli_diff_without_changes() {
    let repo = TempRepo::committed_before();
    let output = stdout(&repo.strata(&["diff", "--no-color"]));
    assert!(
        output.starts_with("no flow changes since HEAD ("),
        "{output}"
    );
    assert_eq!(output.lines().count(), 1);
}

#[test]
fn cli_diff_ignores_hidden_flow_files() {
    let repo = TempRepo::committed_before();
    fs::write(
        repo.flow_path(".hidden.md"),
        "# Hidden\n\n```mermaid\nflowchart TD\n    a --> b\n```\n",
    )
    .unwrap();
    let hidden_path = format!("{FLOWS_DIR}/.hidden.md");
    repo.git(&["add", "-f", &hidden_path]);
    repo.git(&["commit", "-qm", "add hidden flow"]);
    fs::write(
        repo.flow_path(".hidden.md"),
        "# Hidden\n\n```mermaid\nflowchart TD\n    a --> c\n```\n",
    )
    .unwrap();
    let result = repo.strata(&["diff", "--no-color"]);
    assert_eq!(result.status.code(), Some(0));
    let output = stdout(&result);
    assert!(
        output.starts_with("no flow changes since HEAD ("),
        "{output}"
    );
    assert!(!output.contains("Hidden"), "{output}");
    assert!(!output.contains(".hidden.md"), "{output}");
}

#[test]
fn cli_diff_outside_git_fails() {
    let repo = TempRepo::new();
    repo.add_fixture("before", "login.md");
    let output = strata_command()
        .arg("diff")
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("strata: git"));
}

#[test]
fn cli_flows_lists_and_renders_one() {
    let run = |args: &[&str]| strata_command().args(args).output().unwrap();
    let output = stdout(&run(&[
        "flows",
        "--flows",
        "tests/fixtures/flows/after",
        "--no-color",
    ]));
    for expected in ["checkout  Checkout", "login     Login", "refund    Refund"] {
        assert!(output.contains(expected), "{output}");
    }
    let output = stdout(&run(&[
        "flows",
        "refund",
        "--flows",
        "tests/fixtures/flows/after",
        "--no-color",
    ]));
    for expected in [
        "Refund",
        "A customer asks for a refund and gets the money back or a reason why not.",
        "Refund to card",
    ] {
        assert!(output.contains(expected), "{output}");
    }
    let output = run(&[
        "flows",
        "unknown",
        "--flows",
        "tests/fixtures/flows/after",
        "--no-color",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("available:"));
}

#[test]
fn cli_guide_prints_convention() {
    let repo = TempRepo::new();
    let output = stdout(&repo.strata(&["guide"]));
    assert!(output.contains("docs/flows/"));
    assert!(output.contains("strata diff main"));
    let output = stdout(&repo.strata(&["guide", "--flows", "flows"]));
    assert!(output.contains("flows/checkout.md"));
    assert!(!output.contains("docs/flows"));
}

#[test]
fn cli_diff_reports_broken_flow_and_continues() {
    let repo = TempRepo::committed_before();
    fs::write(
        repo.flow_path("checkout.md"),
        "# Checkout\n\n```mermaid\ngraph TD\na -->\n```\n",
    )
    .unwrap();
    repo.add_fixture("after", "refund.md");
    let output = repo.strata(&["diff", "--no-color"]);
    assert_eq!(output.status.code(), Some(1));
    let report = String::from_utf8(output.stdout).unwrap();
    assert!(
        report.contains("Checkout (checkout.md)  parse error, line 5:"),
        "{report}"
    );
    assert!(
        report.contains("Refund (refund.md)  new flow: +4 steps, +3 links"),
        "{report}"
    );
    assert!(report.contains("Refund to card"), "{report}");
}

#[test]
fn cli_diff_handles_catalog_absent_at_base_and_unusual_paths() {
    const UNUSUAL_CATALOG_DIR: &str = "docs/flow catalog";
    let repo = TempRepo::new();
    fs::write(repo.file("README.md"), "# Project\n").unwrap();
    repo.git(&["init", "-q", "-b", BASE_BRANCH]);
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "initial project"]);
    let dir = repo.file(UNUSUAL_CATALOG_DIR);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(
        "tests/fixtures/flows/before/login.md",
        dir.join("sign in ü.md"),
    )
    .unwrap();
    let output = stdout(&repo.strata(&["diff", "--flows", UNUSUAL_CATALOG_DIR, "--no-color"]));
    assert!(output.contains("0 modified, 1 new, 0 removed"), "{output}");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "add catalog"]);
    let output = stdout(&repo.strata(&["diff", "--flows", UNUSUAL_CATALOG_DIR, "--no-color"]));
    assert!(
        output.starts_with("no flow changes since HEAD ("),
        "{output}"
    );
}

#[test]
fn buses_group_only_edges_with_the_same_mark() {
    let mut diagram = strata::parser::parse("graph TD\na --> b\na --> c\na --> d").unwrap();
    diagram.edges[0].mark = strata::Mark::Added;
    diagram.edges[1].mark = strata::Mark::Removed;
    diagram.edges[2].mark = strata::Mark::Added;
    let layout = strata::layout::layout(&diagram);
    let source = layout
        .items
        .iter()
        .position(|item| matches!(item.kind, strata::layout::ItemKind::Node(0)))
        .unwrap();
    let units = layout.out_units(&diagram, source);
    assert_eq!(units.len(), 2);
    assert_eq!(units[0].mark, strata::Mark::Added);
    assert_eq!(units[0].segs.len(), 2);
    assert_eq!(units[1].mark, strata::Mark::Removed);
    assert_eq!(units[1].segs.len(), 1);
}

#[test]
fn cli_render_markdown_and_reject_invalid_arguments() {
    let repo = TempRepo::new();
    fs::write(
        repo.file("flow.markdown"),
        "# Render\n```mermaid\ngraph TD\na[Visible]\n```\n",
    )
    .unwrap();
    let output = stdout(&repo.strata(&["flow.markdown", "--no-color"]));
    assert!(output.starts_with("Render\n"), "{output}");
    assert!(output.contains("Visible"), "{output}");
    fs::copy(repo.file("flow.markdown"), repo.file("README.MD")).unwrap();
    let output = stdout(&repo.strata(&["README.MD", "--no-color"]));
    assert!(output.starts_with("Render\n"), "{output}");
    assert!(output.contains("Visible"), "{output}");
    fs::write(repo.file("empty.md"), "# No diagram\n").unwrap();
    for (args, expected) in [
        (vec!["empty.md"], "empty.md: no mermaid blocks"),
        (vec!["--flows"], "--flows expects a directory"),
        (
            vec!["--watch", "diff"],
            "--watch only applies to rendering a file",
        ),
        (
            vec!["--watch", "flows"],
            "--watch only applies to rendering a file",
        ),
        (
            vec!["--watch", "guide"],
            "--watch only applies to rendering a file",
        ),
        (vec!["guide", "extra"], "USAGE:"),
        (vec!["diff", "HEAD", "extra"], "USAGE:"),
        (vec!["one.mmd", "two.mmd"], "USAGE:"),
    ] {
        let output = repo.strata(&args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
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
