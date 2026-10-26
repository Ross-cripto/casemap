mod common;

use casemap::{Direction, Options, diff, flows};
use common::{CliRun, TempRepo, divergent_catalog, run_casemap, transcript};
use std::fs;
use std::path::Path;

fn matrix(mut render: impl FnMut(&Options) -> String) -> String {
    let mut body = String::new();
    for (label, direction) in [
        ("None", None),
        ("TD", Some(Direction::TopDown)),
        ("BT", Some(Direction::BottomUp)),
        ("LR", Some(Direction::LeftRight)),
        ("RL", Some(Direction::RightLeft)),
    ] {
        for (mode, ascii, color) in [
            ("unicode", false, false),
            ("ascii", true, false),
            ("color", false, true),
        ] {
            let output = render(&Options {
                direction,
                ascii,
                color,
            });
            body.push_str(&format!("== {label} {mode}\n{output}"));
        }
    }
    body.push_str("[end]\n");
    body.replace('\x1b', "␛")
}

fn render_fixture(path: &str) -> String {
    let source = fs::read_to_string(path).unwrap();
    matrix(|options| casemap::render_str(&source, options).unwrap())
}

fn union(name: &str, added: bool) -> String {
    let previous = flows::load_dir(Path::new("tests/fixtures/flows/before")).unwrap();
    let current = flows::load_dir(Path::new("tests/fixtures/flows/after")).unwrap();
    let difference = if added {
        let catalog = if name == "legacy-export" {
            &previous
        } else {
            &current
        };
        let flow = catalog.iter().find(|flow| flow.name == name).unwrap();
        diff::all_added(&flows::diagram(flow).unwrap().unwrap())
    } else {
        let previous = previous.iter().find(|flow| flow.name == name).unwrap();
        let current = current.iter().find(|flow| flow.name == name).unwrap();
        diff::diff(
            &flows::diagram(previous).unwrap().unwrap(),
            &flows::diagram(current).unwrap().unwrap(),
        )
    };
    matrix(|options| casemap::render_diagram(&difference.union, options))
}

fn cli(args: &[&str], env: &[(&str, &str)], input: Option<&str>) -> String {
    transcript(
        &args.join(" "),
        &run_casemap(args, Path::new(env!("CARGO_MANIFEST_DIR")), env, input),
    )
}

fn redact_shas(repo: &TempRepo, mut body: String) -> String {
    for query in [
        &["rev-parse", "HEAD"][..],
        &["rev-parse", "main"],
        &["merge-base", "HEAD", "main"],
    ] {
        let output = common::git_command(&repo.config_path())
            .args(query)
            .current_dir(repo.path())
            .output()
            .unwrap();
        if !output.status.success() {
            continue;
        }
        let sha = String::from_utf8(output.stdout).unwrap();
        body = body.replace(&sha[..flows::SHORT_SHA_LEN], "[SHA]");
    }
    body
}

fn repo_transcript(repo: &TempRepo, args: &[&str]) -> String {
    let run: CliRun = repo.casemap(args).into();
    redact_shas(repo, transcript(&args.join(" "), &run))
}

#[test]
fn render_architecture() {
    insta::assert_snapshot!(
        "render__architecture",
        render_fixture("examples/architecture.mmd")
    );
}

#[test]
fn render_simple() {
    insta::assert_snapshot!("render__simple", render_fixture("examples/simple.mmd"));
}

#[test]
fn render_edgecases() {
    insta::assert_snapshot!(
        "render__edgecases",
        render_fixture("tests/fixtures/edgecases.mmd")
    );
}

#[test]
fn render_fanout() {
    insta::assert_snapshot!(
        "render__fanout",
        render_fixture("tests/fixtures/fanout.mmd")
    );
}

#[test]
fn render_nested() {
    insta::assert_snapshot!(
        "render__nested",
        render_fixture("tests/fixtures/nested.mmd")
    );
}

#[test]
fn render_pipeline() {
    insta::assert_snapshot!(
        "render__pipeline",
        render_fixture("tests/fixtures/pipeline.mmd")
    );
}

#[test]
fn union_checkout() {
    insta::assert_snapshot!("union__checkout", union("checkout", false));
}

#[test]
fn union_login() {
    insta::assert_snapshot!("union__login", union("login", false));
}

#[test]
fn union_refund_new() {
    insta::assert_snapshot!("union__refund_new", union("refund", true));
}

#[test]
fn union_legacy_export_new() {
    insta::assert_snapshot!("union__legacy_export_new", union("legacy-export", true));
}

#[test]
fn markdown_blocks() {
    let document = "# A\nProse\n```mermaid\ngraph TD\na[First] --> b[Second]\n```\n## B\n```bash\n# not a heading\n```\n```mermaid\ngraph LR\nc[Third] --> d[Fourth]\n```\n";
    let mut body = String::new();
    for color in [false, true] {
        body.push_str(&format!(
            "== color={color}\n{}",
            casemap::render_markdown(
                document,
                &Options {
                    color,
                    ..Options::default()
                }
            )
            .unwrap()
        ));
    }
    body.push_str("[end]\n");
    insta::assert_snapshot!("markdown__blocks", body.replace('\x1b', "␛"));
}

#[test]
fn cli_render() {
    insta::assert_snapshot!(
        "cli__render",
        cli(&["examples/simple.mmd", "--no-color"], &[], None)
    );
}

#[test]
fn cli_lowercase_dir() {
    insta::assert_snapshot!(
        "cli__lowercase_dir",
        cli(
            &[
                "--dir",
                "lr",
                "--ascii",
                "--no-color",
                "examples/simple.mmd"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_tb() {
    insta::assert_snapshot!(
        "cli__tb",
        cli(
            &["--dir", "TB", "--no-color", "examples/simple.mmd"],
            &[],
            None
        )
    );
}

#[test]
fn cli_repeated_dir() {
    insta::assert_snapshot!(
        "cli__repeated_dir",
        cli(
            &[
                "--dir",
                "LR",
                "--dir",
                "TD",
                "--no-color",
                "examples/simple.mmd"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_color() {
    insta::assert_snapshot!(
        "cli__color",
        cli(&["--color", "examples/simple.mmd"], &[], None)
    );
}

#[test]
fn cli_color_then_no_color() {
    insta::assert_snapshot!(
        "cli__color_then_no_color",
        cli(&["--color", "--no-color", "examples/simple.mmd"], &[], None)
    );
}

#[test]
fn cli_no_color_then_color() {
    insta::assert_snapshot!(
        "cli__no_color_then_color",
        cli(&["--no-color", "--color", "examples/simple.mmd"], &[], None)
    );
}

// pins: an explicit --color beats NO_COLOR
#[test]
fn cli_no_color_env() {
    insta::assert_snapshot!(
        "cli__no_color_env",
        cli(
            &["--color", "examples/simple.mmd"],
            &[("NO_COLOR", "1")],
            None
        )
    );
}

#[test]
fn cli_stdin() {
    insta::assert_snapshot!(
        "cli__stdin",
        cli(
            &["--no-color"],
            &[],
            Some(include_str!("../examples/simple.mmd"))
        )
    );
}

#[test]
fn cli_stdin_dash() {
    insta::assert_snapshot!(
        "cli__stdin_dash",
        cli(
            &["-", "--no-color"],
            &[],
            Some(include_str!("../examples/simple.mmd"))
        )
    );
}

#[test]
fn cli_stdin_error() {
    insta::assert_snapshot!(
        "cli__stdin_error",
        cli(&["--no-color"], &[], Some("graph TD\n a -->"))
    );
}

#[test]
fn cli_list() {
    insta::assert_snapshot!(
        "cli__list",
        cli(
            &[
                "flows",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_refund() {
    insta::assert_snapshot!(
        "cli__refund",
        cli(
            &[
                "flows",
                "refund",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_unknown_flow() {
    insta::assert_snapshot!(
        "cli__unknown_flow",
        cli(
            &[
                "flows",
                "nosuch",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_missing_catalog() {
    insta::assert_snapshot!(
        "cli__missing_catalog",
        cli(&["flows", "--flows", "nope", "--no-color"], &[], None)
    );
}

#[test]
fn cli_guide() {
    insta::assert_snapshot!("cli__guide", cli(&["guide", "--no-color"], &[], None));
}

#[test]
fn cli_guide_custom() {
    insta::assert_snapshot!(
        "cli__guide_custom",
        cli(&["guide", "--flows", "flows", "--no-color"], &[], None)
    );
}

#[test]
fn cli_watch_no_path() {
    insta::assert_snapshot!("cli__watch_no_path", cli(&["-w"], &[], None));
}

#[test]
fn cli_watch_stdin() {
    insta::assert_snapshot!("cli__watch_stdin", cli(&["-w", "-"], &[], None));
}

#[test]
fn cli_watch_diff() {
    insta::assert_snapshot!("cli__watch_diff", cli(&["--watch", "diff"], &[], None));
}

#[test]
fn cli_markdown() {
    let repo = TempRepo::new();
    for filename in ["README.MD", "flow.markdown"] {
        fs::write(
            repo.file(filename),
            "# Render\n```mermaid\ngraph TD\na[Visible]\n```\n",
        )
        .unwrap();
    }
    fs::write(repo.file("empty.md"), "# No diagram\n").unwrap();
    insta::assert_snapshot!(
        "cli__markdown_uppercase",
        repo_transcript(&repo, &["README.MD", "--no-color"])
    );
    insta::assert_snapshot!(
        "cli__markdown_extension",
        repo_transcript(&repo, &["flow.markdown", "--no-color"])
    );
    insta::assert_snapshot!(
        "cli__markdown_empty",
        repo_transcript(&repo, &["empty.md", "--no-color"])
    );
}

#[test]
fn cli_catalog_diff() {
    let repo = divergent_catalog();
    insta::assert_snapshot!(
        "cli__diff_main",
        repo_transcript(&repo, &["diff", "main", "--no-color"])
    );
    insta::assert_snapshot!(
        "cli__diff_head",
        repo_transcript(&repo, &["diff", "--no-color"])
    );
    insta::assert_snapshot!(
        "cli__diff_color",
        repo_transcript(&repo, &["diff", "main", "--color"])
    );
    insta::assert_snapshot!(
        "cli__diff_invalid_revision",
        repo_transcript(&repo, &["diff", "-", "--no-color"])
    );
}

#[test]
fn cli_broken_flow() {
    let repo = TempRepo::committed_before();
    fs::write(
        repo.flow_path("checkout.md"),
        "# Checkout\n\n```mermaid\ngraph TD\na -->\n```\n",
    )
    .unwrap();
    repo.add_fixture("after", "refund.md");
    insta::assert_snapshot!(
        "cli__diff_broken_flow",
        repo_transcript(&repo, &["diff", "--no-color"])
    );
}

#[test]
fn cli_empty_catalog() {
    let repo = TempRepo::new();
    fs::create_dir(repo.file("empty")).unwrap();
    insta::assert_snapshot!(
        "cli__empty_catalog",
        repo_transcript(&repo, &["flows", "--flows", "empty", "--no-color"])
    );
}

#[test]
fn cli_no_color_env_no_flag() {
    insta::assert_snapshot!(
        "cli__no_color_env_no_flag",
        cli(&["examples/simple.mmd"], &[("NO_COLOR", "1")], None)
    );
}

#[test]
fn cli_no_flags_piped() {
    insta::assert_snapshot!(
        "cli__no_flags_piped",
        cli(&["examples/simple.mmd"], &[], None)
    );
}

#[test]
fn cli_flows_flag_before_subcommand() {
    insta::assert_snapshot!(
        "cli__flows_flag_before_subcommand",
        cli(
            &[
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color",
                "flows"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_flows_repeated() {
    insta::assert_snapshot!(
        "cli__flows_repeated",
        cli(
            &[
                "--flows",
                "nope",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color",
                "flows"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_flows_ascii_dir() {
    insta::assert_snapshot!(
        "cli__flows_ascii_dir",
        cli(
            &[
                "flows",
                "refund",
                "--flows",
                "tests/fixtures/flows/after",
                "--ascii",
                "--dir",
                "LR",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_diff_ascii() {
    let repo = divergent_catalog();
    insta::assert_snapshot!(
        "cli__diff_ascii",
        repo_transcript(&repo, &["diff", "main", "--ascii", "--no-color"])
    );
}

fn catalog_with_check_problems() -> TempRepo {
    let repo = TempRepo::new();
    fs::write(repo.flow_path("Bad.md"), "# Checkout\n").unwrap();
    repo
}

#[test]
fn cli_check_text() {
    let repo = catalog_with_check_problems();
    insta::assert_snapshot!(
        "cli__check_text",
        repo_transcript(&repo, &["check", "--no-color"])
    );
}

#[test]
fn cli_check_json() {
    let repo = catalog_with_check_problems();
    insta::assert_snapshot!(
        "cli__check_json",
        repo_transcript(&repo, &["check", "--format", "json"])
    );
}

#[test]
fn cli_diff_stale() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "fn pay() { changed(); }\n").unwrap();
    insta::assert_snapshot!(
        "cli__diff_stale",
        repo_transcript(&repo, &["diff", "--no-color"])
    );
}

#[test]
fn cli_which() {
    let repo = common::anchored_catalog();
    insta::assert_snapshot!(
        "cli__which",
        repo_transcript(&repo, &["which", "src/pay.rs", "src/other.rs"])
    );
}

#[test]
fn diff_markdown() {
    let repo = divergent_catalog();
    insta::assert_snapshot!(
        "diff_markdown",
        repo_transcript(&repo, &["diff", "main", "--format", "markdown"])
    );
}

#[test]
fn diff_markdown_stale() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "fn pay() { changed(); }\n").unwrap();
    insta::assert_snapshot!(
        "diff_markdown_stale",
        repo_transcript(&repo, &["diff", "--format", "markdown"])
    );
}

#[test]
fn diff_markdown_quiet() {
    let repo = common::anchored_catalog();
    insta::assert_snapshot!(
        "diff_markdown_quiet",
        repo_transcript(&repo, &["diff", "--format", "markdown"])
    );
}

#[test]
fn diff_markdown_broken() {
    let repo = common::anchored_catalog();
    fs::write(
        repo.flow_path("broken.md"),
        "# Broken\n\nDescription.\n\n```mermaid\ngraph TD\na -->\n```\n",
    )
    .unwrap();
    insta::assert_snapshot!(
        "diff_markdown_broken",
        repo_transcript(&repo, &["diff", "--format", "markdown"])
    );
}

#[test]
fn coverage_repository_text() {
    let repo = common::coverage_catalog();
    insta::assert_snapshot!(
        "coverage_repository_text",
        repo_transcript(&repo, &["coverage"])
    );
}

#[test]
fn coverage_branch_text() {
    let repo = common::coverage_catalog();
    insta::assert_snapshot!(
        "coverage_branch_text",
        repo_transcript(&repo, &["coverage", "main"])
    );
}

#[test]
fn diff_markdown_uncovered() {
    let repo = common::coverage_catalog();
    insta::assert_snapshot!(
        "diff_markdown_uncovered",
        repo_transcript(&repo, &["diff", "main", "--format", "markdown"])
    );
}
