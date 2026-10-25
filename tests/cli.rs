mod common;

use assert_cmd::Command;
use common::{BASE_BRANCH, FLOWS_DIR, TempRepo, divergent_catalog};
use predicates::prelude::*;
use std::fs;
use std::process::Output;
use strata::flows;

fn strata_command(repo: &TempRepo) -> Command {
    let config = repo.path().parent().unwrap().join("gitconfig");
    let isolated = common::strata_command(&config);
    let mut command = Command::new(env!("CARGO_BIN_EXE_strata"));
    for (variable, value) in isolated.get_envs() {
        match value {
            Some(value) => {
                command.env(variable, value);
            }
            None => {
                command.env_remove(variable);
            }
        }
    }
    command
}

fn run_repo(repo: &TempRepo, args: &[&str]) -> Output {
    strata_command(repo)
        .args(args)
        .current_dir(repo.path())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn cli_diff_uses_merge_base_not_ref() {
    let repo = divergent_catalog();
    let sha = repo.git_stdout(&["merge-base", BASE_BRANCH, "HEAD"]);
    let output = stdout(&run_repo(&repo, &["diff", BASE_BRANCH, "--no-color"]));
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
    let output = stdout(&run_repo(&repo, &["diff", "--no-color"]));
    assert!(
        output.contains("X (x.md)  new flow: +1 step (base version did not parse)"),
        "{output}"
    );
}

#[test]
fn cli_diff_against_branch_includes_uncommitted() {
    let repo = divergent_catalog();
    let output = stdout(&run_repo(&repo, &["diff", BASE_BRANCH, "--no-color"]));
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
    let output = stdout(&run_repo(&repo, &["diff", "--no-color"]));
    assert!(output.contains("1 modified, 0 new, 0 removed"), "{output}");
}

#[test]
fn cli_diff_without_changes() {
    let repo = TempRepo::committed_before();
    let output = stdout(&run_repo(&repo, &["diff", "--no-color"]));
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
    let result = run_repo(&repo, &["diff", "--no-color"]);
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
    let output = strata_command(&repo)
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
    let repo = TempRepo::new();
    let run = |args: &[&str]| strata_command(&repo).args(args).output().unwrap();
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
    assert!(predicate::str::contains("available:").eval(&String::from_utf8_lossy(&output.stderr)));
}

#[test]
fn cli_guide_prints_convention() {
    let repo = TempRepo::new();
    let output = stdout(&run_repo(&repo, &["guide"]));
    assert!(output.contains("docs/flows/"));
    assert!(output.contains("strata diff main"));
    let output = stdout(&run_repo(&repo, &["guide", "--flows", "flows"]));
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
    let output = run_repo(&repo, &["diff", "--no-color"]);
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
    let output = stdout(&run_repo(
        &repo,
        &["diff", "--flows", UNUSUAL_CATALOG_DIR, "--no-color"],
    ));
    assert!(output.contains("0 modified, 1 new, 0 removed"), "{output}");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "add catalog"]);
    let output = stdout(&run_repo(
        &repo,
        &["diff", "--flows", UNUSUAL_CATALOG_DIR, "--no-color"],
    ));
    assert!(
        output.starts_with("no flow changes since HEAD ("),
        "{output}"
    );
}

#[test]
fn cli_render_markdown_and_reject_invalid_arguments() {
    let repo = TempRepo::new();
    fs::write(
        repo.file("flow.markdown"),
        "# Render\n```mermaid\ngraph TD\na[Visible]\n```\n",
    )
    .unwrap();
    let output = stdout(&run_repo(&repo, &["flow.markdown", "--no-color"]));
    assert!(output.starts_with("Render\n"), "{output}");
    assert!(output.contains("Visible"), "{output}");
    fs::copy(repo.file("flow.markdown"), repo.file("README.MD")).unwrap();
    let output = stdout(&run_repo(&repo, &["README.MD", "--no-color"]));
    assert!(output.starts_with("Render\n"), "{output}");
    assert!(output.contains("Visible"), "{output}");
    fs::write(repo.file("empty.md"), "# No diagram\n").unwrap();
    for (args, expected) in [
        (vec!["empty.md"], "empty.md: no mermaid blocks"),
        (vec!["--flows"], "--flows"),
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
        (vec!["guide", "extra"], "Usage:"),
        (vec!["diff", "HEAD", "extra"], "Usage:"),
        (vec!["one.mmd", "two.mmd"], "Usage:"),
    ] {
        let output = run_repo(&repo, &args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn usage_errors_exit_with_one() {
    let repo = TempRepo::new();
    for args in [
        vec!["--bogus"],
        vec!["--flows"],
        vec!["--dir"],
        vec!["--dir", "XX"],
        vec!["a.mmd", "b.mmd"],
    ] {
        strata_command(&repo).args(args).assert().code(1);
    }
    for flag in ["--help", "--version", "-h", "-V"] {
        strata_command(&repo).arg(flag).assert().success();
    }
}

#[test]
fn completions_generate_for_each_shell() {
    let repo = TempRepo::new();
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        strata_command(&repo)
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::is_empty().not());
    }
    strata_command(&repo)
        .args(["completions", "nosuch"])
        .assert()
        .code(1);
}

#[test]
fn file_and_subcommand_conflict() {
    let repo = TempRepo::new();
    strata_command(&repo)
        .args(["x.mmd", "--ascii", "diff"])
        .assert()
        .code(1)
        .stderr("strata: FILE cannot be combined with a subcommand\n");
}

#[test]
fn dot_slash_renders_a_file_named_like_a_command() {
    let repo = TempRepo::new();
    fs::write(repo.file("diff"), "graph TD\na[Visible]\n").unwrap();
    for args in [
        vec!["./diff", "--no-color"],
        vec!["--no-color", "--", "diff"],
    ] {
        strata_command(&repo)
            .current_dir(repo.path())
            .args(args)
            .assert()
            .success()
            .stdout(predicate::str::contains("Visible"));
    }
}

#[test]
fn no_color_wins_across_subcommand_boundaries() {
    let repo = TempRepo::new();
    for args in [
        ["--color", "flows", "refund", "--no-color"],
        ["--no-color", "flows", "refund", "--color"],
    ] {
        strata_command(&repo)
            .args(args)
            .args(["--flows", "tests/fixtures/flows/after"])
            .assert()
            .success()
            .stdout(predicate::str::contains("\x1b").not());
    }
}

const WATCH_TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);
const WATCH_SAVE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(300);

struct WatchProcess {
    child: std::process::Child,
    output: std::sync::mpsc::Receiver<Vec<u8>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl WatchProcess {
    fn start(repo: &TempRepo) -> Self {
        use std::io::Read;
        use std::process::Stdio;

        let mut command = common::strata_command(&repo.path().parent().unwrap().join("gitconfig"));
        let mut child = command
            .args(["--watch", "--no-color", "f.mmd"])
            .current_dir(repo.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (sender, output) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            loop {
                let count = stdout.read(&mut buffer).unwrap();
                if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            output,
            reader: Some(reader),
        }
    }

    fn wait_for(&self, expected: &str, mut on_tick: impl FnMut()) -> bool {
        let deadline = std::time::Instant::now() + WATCH_TEST_DEADLINE;
        let mut output = Vec::new();
        while std::time::Instant::now() < deadline {
            match self.output.recv_timeout(WATCH_SAVE_INTERVAL) {
                Ok(chunk) => output.extend(chunk),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => on_tick(),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if String::from_utf8_lossy(&output).contains(expected) {
                return true;
            }
        }
        false
    }
}

impl Drop for WatchProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.child.wait().unwrap();
        self.reader.take().unwrap().join().unwrap();
    }
}

#[test]
fn cli_watch_redraws_on_save() {
    let repo = TempRepo::new();
    fs::write(repo.file("f.mmd"), "graph TD\na[Initial]\n").unwrap();
    let watch = WatchProcess::start(&repo);
    assert!(
        watch.wait_for("Initial", || {}),
        "missing initial watch frame"
    );
    let mut revision = 0;
    let updated = watch.wait_for("SavedLabel", || {
        revision += 1;
        fs::write(
            repo.file("f.mmd"),
            format!("graph TD\na[SavedLabel{revision}]\n"),
        )
        .unwrap();
    });
    drop(watch);
    assert!(
        updated,
        "watch did not render a saved label within 15 seconds"
    );
}

#[test]
fn cli_watch_draws_missing_file_immediately() {
    let repo = TempRepo::new();
    let watch = WatchProcess::start(&repo);
    let displayed = watch.wait_for("cannot read file:", || {});
    drop(watch);
    assert!(displayed, "watch did not display the missing-file error");
}
