mod common;

use assert_cmd::Command;
use casemap::{flows, skill};
use common::{BASE_BRANCH, FLOWS_DIR, TempRepo, divergent_catalog};
use predicates::prelude::*;
use std::fs;
use std::io::Read;
use std::process::{Child, Output, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

fn casemap_command(repo: &TempRepo) -> Command {
    Command::from_std(common::casemap_command(&repo.config_path()))
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
    let output = stdout(&repo.casemap(&["diff", BASE_BRANCH, "--no-color"]));
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
    let output = stdout(&repo.casemap(&["diff", "--no-color"]));
    assert!(
        output.contains("X (x.md)  new flow: +1 step (base version did not parse)"),
        "{output}"
    );
}

#[test]
fn cli_diff_against_branch_includes_uncommitted() {
    let repo = divergent_catalog();
    let output = stdout(&repo.casemap(&["diff", BASE_BRANCH, "--no-color"]));
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
    let output = stdout(&repo.casemap(&["diff", "--no-color"]));
    assert!(output.contains("1 modified, 0 new, 0 removed"), "{output}");
}

#[test]
fn cli_diff_without_changes() {
    let repo = TempRepo::committed_before();
    let output = stdout(&repo.casemap(&["diff", "--no-color"]));
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
    let result = repo.casemap(&["diff", "--no-color"]);
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
    let output = casemap_command(&repo)
        .arg("diff")
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("casemap: git"));
}

#[test]
fn cli_flows_lists_and_renders_one() {
    let repo = TempRepo::new();
    let run = |args: &[&str]| casemap_command(&repo).args(args).output().unwrap();
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
    let output = stdout(&repo.casemap(&["guide"]));
    assert!(output.contains("docs/flows/"));
    assert!(output.contains("casemap diff main"));
    let output = stdout(&repo.casemap(&["guide", "--flows", "flows"]));
    assert!(output.contains("flows/check-out-a-cart.md"));
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
    let output = repo.casemap(&["diff", "--no-color"]);
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
    let output = stdout(&repo.casemap(&["diff", "--flows", UNUSUAL_CATALOG_DIR, "--no-color"]));
    assert!(output.contains("0 modified, 1 new, 0 removed"), "{output}");
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "add catalog"]);
    let output = stdout(&repo.casemap(&["diff", "--flows", UNUSUAL_CATALOG_DIR, "--no-color"]));
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
    let output = stdout(&repo.casemap(&["flow.markdown", "--no-color"]));
    assert!(output.starts_with("Render\n"), "{output}");
    assert!(output.contains("Visible"), "{output}");
    fs::copy(repo.file("flow.markdown"), repo.file("README.MD")).unwrap();
    let output = stdout(&repo.casemap(&["README.MD", "--no-color"]));
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
        let output = repo.casemap(&args);
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
        vec!["a.mmd", "b.mmd"],
    ] {
        casemap_command(&repo).args(args).assert().code(1);
    }
    casemap_command(&repo)
        .args(["--dir", "XX"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("expects one of TD, BT, LR, RL"));
    casemap_command(&repo)
        .args(["--dir=lr", "examples/simple.mmd", "--no-color"])
        .assert()
        .success();
    for flag in ["--help", "--version", "-h", "-V"] {
        casemap_command(&repo).arg(flag).assert().success();
    }
}

#[test]
fn completions_generate_for_each_shell() {
    let repo = TempRepo::new();
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        casemap_command(&repo)
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::is_empty().not());
    }
    casemap_command(&repo)
        .args(["completions", "nosuch"])
        .assert()
        .code(1);
}

#[test]
fn file_and_subcommand_conflict() {
    let repo = TempRepo::new();
    casemap_command(&repo)
        .args(["x.mmd", "--ascii", "diff"])
        .assert()
        .code(1)
        .stderr("casemap: FILE cannot be combined with a subcommand\n");
}

#[test]
fn dot_slash_renders_a_file_named_like_a_command() {
    let repo = TempRepo::new();
    fs::write(repo.file("diff"), "graph TD\na[Visible]\n").unwrap();
    for args in [
        vec!["./diff", "--no-color"],
        vec!["--no-color", "--", "diff"],
    ] {
        casemap_command(&repo)
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
        casemap_command(&repo)
            .args(args)
            .args(["--flows", "tests/fixtures/flows/after"])
            .assert()
            .success()
            .stdout(predicate::str::contains("\x1b").not());
    }
}

const WATCH_TEST_DEADLINE: Duration = Duration::from_secs(15);
const WATCH_SAVE_INTERVAL: Duration = Duration::from_millis(300);

struct WatchProcess {
    child: Child,
    output: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
}

impl WatchProcess {
    fn start(repo: &TempRepo) -> Self {
        let mut command = common::casemap_command(&repo.config_path());
        let mut child = command
            .args(["--watch", "--no-color", "f.mmd"])
            .current_dir(repo.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut buffer = [0; 4096];
            loop {
                let Ok(count) = stdout.read(&mut buffer) else {
                    break;
                };
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

    // Each call starts with an empty buffer so a second wait only sees new output.
    fn wait_for(&self, expected: &str, mut on_tick: impl FnMut()) -> bool {
        let deadline = Instant::now() + WATCH_TEST_DEADLINE;
        let mut output = Vec::new();
        while Instant::now() < deadline {
            match self.output.recv_timeout(WATCH_SAVE_INTERVAL) {
                Ok(chunk) => output.extend(chunk),
                Err(RecvTimeoutError::Timeout) => on_tick(),
                Err(RecvTimeoutError::Disconnected) => break,
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
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
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

#[test]
fn check_passes_on_a_valid_catalog() {
    let repo = TempRepo::new();
    repo.copy_catalog("after");
    assert_eq!(
        stdout(&repo.casemap(&["check", "--no-color"])),
        "checked 3 flows: ok\n"
    );
}

#[test]
fn check_exit_codes() {
    check_exits_1_on_errors();
    check_warnings_exit_0_unless_strict();
    check_empty_catalog_warns();
}

fn write_valid_flow(repo: &TempRepo, name: &str) {
    let fixture = fs::read_to_string("tests/fixtures/flows/after/checkout.md").unwrap();
    fs::write(repo.flow_path(name), fixture).unwrap();
}

#[test]
fn check_exits_1_on_errors() {
    let repo = TempRepo::new();
    write_valid_flow(&repo, "checkout.md");
    let path = repo.flow_path("checkout.md");
    let valid = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        valid.replace(
            "cart([Customer opens cart]) --> pay{Payment ok?}",
            "cart -->",
        ),
    )
    .unwrap();
    let output = repo.casemap(&["check", "--no-color"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("[parse-error]"), "{text}");
    assert!(text.contains("checked 1 flows: 1 error"), "{text}");
}

#[test]
fn check_warnings_exit_0_unless_strict() {
    let repo = TempRepo::new();
    write_valid_flow(&repo, "checkout.md");
    let path = repo.flow_path("checkout.md");
    let valid = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{valid}```mermaid\nflowchart TD\n```\n")).unwrap();
    let report = stdout(&repo.casemap(&["check", "--no-color"]));
    assert!(report.contains("[extra-diagram]"), "{report}");
    assert!(report.ends_with("checked 1 flows: 1 warning\n"), "{report}");
    let strict = repo.casemap(&["check", "--strict", "--no-color"]);
    assert_eq!(strict.status.code(), Some(1));
    assert!(strict.stderr.is_empty());
    assert_eq!(String::from_utf8(strict.stdout).unwrap(), report);
}

#[test]
fn check_empty_catalog_warns() {
    let repo = TempRepo::new();
    assert_eq!(
        stdout(&repo.casemap(&["check", "--no-color"])),
        "docs/flows: warning: no flows yet [empty-catalog]\nchecked 0 flows: 1 warning\n"
    );
    let strict = repo.casemap(&["check", "--strict"]);
    assert_eq!(strict.status.code(), Some(1));
    assert!(strict.stderr.is_empty());
    assert!(String::from_utf8_lossy(&strict.stdout).contains("[empty-catalog]"));
}

#[test]
fn check_json_shape() {
    let repo = TempRepo::new();
    fs::write(
        repo.flow_path("checkout.md"),
        "# Checkout\n\nDescription.\n```mermaid\nflowchart TD\na -->\n```\n",
    )
    .unwrap();
    let output = repo.casemap(&["check", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "version": 1, "catalog": "docs/flows", "flows": 1, "errors": 1, "warnings": 0,
            "problems": [{"path": "docs/flows/checkout.md", "line": 6, "severity": "error", "code": "parse-error", "message": "link has no target node"}]
        })
    );
    fs::remove_file(repo.flow_path("checkout.md")).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&repo.casemap(&["check", "--format", "json"]))).unwrap();
    assert!(value["problems"][0]["path"].is_null());
    assert!(value["problems"][0]["line"].is_null());
    assert_eq!(value["problems"][0]["severity"], "warning");
}

#[test]
fn check_text_points_at_file_and_line() {
    let repo = TempRepo::new();
    fs::write(
        repo.flow_path("checkout.md"),
        "# Checkout\n\n```mermaid\ngraph TD\na -->\n```\n",
    )
    .unwrap();
    let output = repo.casemap(&["check", "--no-color"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("docs/flows/checkout.md:5: error: link has no target node [parse-error]\n")
    );
    let output = repo.casemap(&["check", "--color", "--flows", "docs/flows/"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("docs/flows/checkout.md:5: \x1b[31merror\x1b[0m:"));
    assert!(!text.contains("flows//"));
}

#[test]
fn flows_json_lists_the_catalog() {
    let repo = TempRepo::new();
    repo.copy_catalog("after");
    let output = stdout(&repo.casemap(&["flows", "--format", "json"]));
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 2);
    assert_eq!(value["version"], 1);
    let entries = value["flows"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[0],
        serde_json::json!({"name": "checkout", "path": "docs/flows/checkout.md", "title": "Checkout", "description": "A customer pays for the cart.", "has_diagram": true})
    );
    let output = repo.casemap(&["flows", "checkout", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--format json lists the catalog; drop the flow name")
    );
}

#[test]
fn check_this_repository_strictly() {
    let repo = TempRepo::new();
    casemap_command(&repo)
        .args(["check", "--strict"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .assert()
        .success()
        .stdout(predicate::str::starts_with("checked ").and(predicate::str::ends_with(": ok\n")));
}

#[test]
fn json_commands_reject_missing_catalog_without_stdout() {
    let repo = TempRepo::new();
    for command in ["check", "flows"] {
        let output = repo.casemap(&[command, "--flows", "absent", "--format", "json"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "casemap: no flow catalog at absent (create it or pass --flows <dir>)\n"
        );
    }
}

fn skill_paths() -> Vec<String> {
    skill::INSTALL_DIRS
        .iter()
        .map(|(directory, _)| format!("{directory}/{}/SKILL.md", skill::SKILL_NAME))
        .collect()
}

fn init_repo() -> TempRepo {
    let repo = TempRepo::new();
    fs::remove_dir_all(repo.file(skill::DEFAULT_CATALOG)).unwrap();
    fs::remove_dir(repo.file("docs")).unwrap();
    repo.git(&["init", "-q"]);
    repo
}

fn installed_skills(repo: &TempRepo) -> Vec<String> {
    skill_paths()
        .iter()
        .map(|path| fs::read_to_string(repo.file(path)).unwrap())
        .collect()
}

#[test]
fn init_creates_catalog_and_installs_skill() {
    let repo = init_repo();
    let output = stdout(&repo.casemap(&["init"]));
    assert!(
        output.starts_with(&format!("created {}/\n", skill::DEFAULT_CATALOG)),
        "{output}"
    );
    assert!(repo.flow_path(".gitkeep").is_file());
    for (directory, agent_label) in skill::INSTALL_DIRS {
        let path = format!("{directory}/{}/SKILL.md", skill::SKILL_NAME);
        assert!(repo.file(&path).is_file());
        assert!(
            output.contains(&format!("installed {path} ({agent_label})")),
            "{output}"
        );
    }
    assert!(output.contains(&format!(
        "Use-case flows live in {}/ (casemap skill)",
        skill::DEFAULT_CATALOG
    )));
}

#[test]
fn init_installs_the_rendered_skill() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render(skill::DEFAULT_CATALOG));
    }
}

#[test]
fn init_renders_a_custom_catalog() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init", "--flows", "flows"]));
    assert!(repo.file("flows/.gitkeep").is_file());
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render("flows"));
        assert!(!text.contains(skill::DEFAULT_CATALOG));
    }
}

#[test]
fn init_is_idempotent() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    let output = stdout(&repo.casemap(&["init"]));
    assert!(output.starts_with(&format!("exists {}/\n", skill::DEFAULT_CATALOG)));
    assert_eq!(output.matches("up to date ").count(), 2);
}

#[test]
fn init_keeps_differing_files_without_force() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    for path in skill_paths() {
        fs::write(repo.file(&path), "custom skill\n").unwrap();
    }
    let output = stdout(&repo.casemap(&["init"]));
    for path in skill_paths() {
        assert_eq!(
            fs::read_to_string(repo.file(&path)).unwrap(),
            "custom skill\n"
        );
        assert!(output.contains(&format!(
            "kept {path} (differs from this casemap version; use --force to update)"
        )));
    }
}

#[test]
fn init_force_overwrites_only_skill_files() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    fs::write(repo.flow_path("custom.md"), "my flow\n").unwrap();
    fs::write(repo.file("AGENTS.md"), "my rules\n").unwrap();
    for path in skill_paths() {
        fs::write(repo.file(&path), "custom skill\n").unwrap();
    }
    let output = stdout(&repo.casemap(&["init", "--force"]));
    assert_eq!(output.matches("updated ").count(), 2);
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render(skill::DEFAULT_CATALOG));
    }
    assert_eq!(
        fs::read_to_string(repo.flow_path("custom.md")).unwrap(),
        "my flow\n"
    );
    assert_eq!(
        fs::read_to_string(repo.file("AGENTS.md")).unwrap(),
        "my rules\n"
    );
}

fn files_under(directory: &std::path::Path, root: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == ".git" {
            continue;
        }
        if path.is_dir() {
            files.extend(files_under(&path, root));
        } else {
            files.push(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    files.sort();
    files
}

#[test]
fn init_writes_only_its_targets() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    let mut expected = skill_paths();
    expected.push(format!("{}/.gitkeep", skill::DEFAULT_CATALOG));
    expected.sort();
    assert_eq!(files_under(repo.path(), repo.path()), expected);
}

#[test]
fn init_treats_crlf_copy_as_up_to_date() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    let crlf = format!(
        "\u{feff}{}",
        skill::render(skill::DEFAULT_CATALOG).replace('\n', "\r\n")
    );
    for path in skill_paths() {
        fs::write(repo.file(&path), &crlf).unwrap();
    }
    let output = stdout(&repo.casemap(&["init", "--force"]));
    assert_eq!(output.matches("up to date ").count(), 2);
    for text in installed_skills(&repo) {
        assert_eq!(text, crlf);
    }
}

#[test]
fn init_from_a_subdirectory_installs_at_the_root() {
    let repo = init_repo();
    fs::create_dir(repo.file("web")).unwrap();
    let output = casemap_command(&repo)
        .arg("init")
        .current_dir(repo.file("web"))
        .output()
        .unwrap();
    let text = stdout(&output);
    let catalog = format!("web/{}", skill::DEFAULT_CATALOG);
    assert!(text.contains(&format!("created {catalog}/")), "{text}");
    assert!(repo.file(&format!("{catalog}/.gitkeep")).is_file());
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render(&catalog));
        assert!(text.contains(&format!(" pass `--flows {catalog}` ")));
    }
    assert!(!repo.file("web/.agents").exists());
}

#[test]
fn init_warns_when_skill_is_gitignored() {
    let repo = init_repo();
    fs::write(repo.file(".gitignore"), ".agents/\n.claude/\n").unwrap();
    let output = stdout(&repo.casemap(&["init"]));
    for path in skill_paths() {
        assert!(
            output.contains(&format!(
                "warning: {path} is gitignored; teammates will not get the skill"
            )),
            "{output}"
        );
    }
}

#[test]
fn init_rejects_absolute_flows_dir() {
    let repo = init_repo();
    let path = repo.file("catalog");
    let output = repo.casemap(&["init", "--flows", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--flows must stay inside the repository")
    );
    assert!(!path.exists());
}

#[test]
fn init_rejects_a_file_as_catalog() {
    let repo = init_repo();
    fs::write(repo.file("flows"), "keep me\n").unwrap();
    let output = repo.casemap(&["init", "--flows", "flows"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("flows exists and is not a directory")
    );
    assert_eq!(fs::read_to_string(repo.file("flows")).unwrap(), "keep me\n");
}

#[test]
fn init_continues_after_failed_writes() {
    let repo = init_repo();
    fs::write(repo.file(".claude"), "block skill directory").unwrap();
    let output = repo.casemap(&["init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error: .claude/skills/casemap/SKILL.md:")
    );
    assert!(repo.file(&skill_paths()[1]).is_file());
    assert!(repo.flow_path(".gitkeep").is_file());
}

#[cfg(unix)]
#[test]
fn init_writes_through_symlinked_skills() {
    let repo = init_repo();
    let shared = repo.file("shared-skill");
    fs::create_dir(&shared).unwrap();
    fs::create_dir_all(repo.file(".agents/skills")).unwrap();
    std::os::unix::fs::symlink(&shared, repo.file(".agents/skills/casemap")).unwrap();
    stdout(&repo.casemap(&["init"]));
    assert!(repo.file(".agents/skills/casemap").is_symlink());
    assert_eq!(
        fs::read_to_string(shared.join("SKILL.md")).unwrap(),
        skill::render(skill::DEFAULT_CATALOG)
    );
}

#[test]
fn init_keeps_non_utf8_skills_until_forced() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    for path in skill_paths() {
        fs::write(repo.file(&path), [0xff, 0xfe]).unwrap();
    }
    let output = stdout(&repo.casemap(&["init"]));
    assert_eq!(output.matches("kept ").count(), 2);
    for path in skill_paths() {
        assert_eq!(fs::read(repo.file(&path)).unwrap(), [0xff, 0xfe]);
    }
    let output = stdout(&repo.casemap(&["init", "--force"]));
    assert_eq!(output.matches("updated ").count(), 2);
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render(skill::DEFAULT_CATALOG));
    }
}

#[test]
fn init_outside_git_sets_up_the_current_directory() {
    let repo = TempRepo::new();
    let mut git = common::git_command(&repo.config_path());
    let location = git
        .args(["rev-parse", "--show-toplevel"])
        .env("GIT_CEILING_DIRECTORIES", repo.path().parent().unwrap())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(!location.status.success());
    let output = casemap_command(&repo)
        .arg("init")
        .env("GIT_CEILING_DIRECTORIES", repo.path().parent().unwrap())
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(stdout(&output).starts_with("note: not a git repository; casemap diff needs git\n"));
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render(skill::DEFAULT_CATALOG));
    }
}

#[cfg(unix)]
#[test]
fn init_reports_the_failed_catalog_file_and_continues() {
    let repo = init_repo();
    fs::write(repo.file("docs"), "block catalog directory").unwrap();
    let output = repo.casemap(&["init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains(&format!("error: {}:", skill::DEFAULT_CATALOG))
    );
    assert_eq!(
        fs::read_to_string(repo.file("docs")).unwrap(),
        "block catalog directory"
    );
    for path in skill_paths() {
        assert!(repo.file(&path).is_file());
    }
}

#[test]
fn init_rejects_paths_outside_the_repo() {
    let repo = init_repo();
    for path in ["../x", "/x"] {
        let output = repo.casemap(&["init", "--flows", path]);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--flows must stay inside the repository")
        );
        assert!(output.stdout.is_empty());
    }
    assert!(!repo.file(".agents").exists());
}

#[test]
fn init_normalises_catalog_path() {
    let repo = init_repo();
    fs::create_dir(repo.file("web")).unwrap();
    let output = casemap_command(&repo)
        .args(["init", "--flows", "./flows"])
        .current_dir(repo.file("web"))
        .output()
        .unwrap();
    assert!(stdout(&output).starts_with("created web/flows/\n"));
    for text in installed_skills(&repo) {
        assert_eq!(text, skill::render("web/flows"));
    }
    let output = casemap_command(&repo)
        .args(["init", "--flows", "../docs/./flows"])
        .current_dir(repo.file("web"))
        .output()
        .unwrap();
    assert!(stdout(&output).starts_with(&format!("created {}/\n", skill::DEFAULT_CATALOG)));
}

#[cfg(unix)]
fn relative_skill_link(repo: &TempRepo) {
    fs::create_dir_all(repo.file(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(
        "../../.agents/skills/casemap",
        repo.file(".claude/skills/casemap"),
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
fn init_reports_dangling_skill_symlink() {
    let repo = init_repo();
    relative_skill_link(&repo);
    let output = repo.casemap(&["init"]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("error: .claude/skills/casemap/SKILL.md:"),
        "{error}"
    );
    assert!(
        error.contains(".claude/skills/casemap is a symlink to a missing path"),
        "{error}"
    );
    assert!(repo.file(".agents/skills/casemap/SKILL.md").is_file());
}

#[cfg(unix)]
#[test]
fn init_writes_through_relative_skill_symlink() {
    let repo = init_repo();
    fs::create_dir_all(repo.file(".agents/skills/casemap")).unwrap();
    relative_skill_link(&repo);
    stdout(&repo.casemap(&["init"]));
    assert_eq!(
        stdout(&repo.casemap(&["init"]))
            .matches("up to date ")
            .count(),
        2
    );
    assert!(repo.file(".claude/skills/casemap").is_symlink());
}

#[test]
fn init_fails_when_git_errors() {
    let repo = init_repo();
    let output = casemap_command(&repo)
        .arg("init")
        .env("PATH", repo.file("no-git"))
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("casemap: cannot run git"));
    assert!(output.stdout.is_empty());
    assert!(!repo.file(".agents").exists());
}

#[test]
fn init_leaves_populated_catalog_alone() {
    let repo = init_repo();
    fs::create_dir_all(repo.file(skill::DEFAULT_CATALOG)).unwrap();
    fs::write(repo.flow_path("a.md"), "existing flow").unwrap();
    stdout(&repo.casemap(&["init"]));
    assert!(!repo.flow_path(".gitkeep").exists());
    assert_eq!(
        fs::read_to_string(repo.flow_path("a.md")).unwrap(),
        "existing flow"
    );
}

#[test]
fn guide_trims_trailing_slash() {
    let repo = TempRepo::new();
    let text = stdout(&repo.casemap(&["guide", "--flows", "flows/"]));
    assert!(text.contains("flows/check-out-a-cart.md"));
    assert!(!text.contains("flows//"));
}

#[cfg(unix)]
#[test]
fn init_rejects_catalog_through_outside_symlink() {
    let repo = init_repo();
    let outside = assert_fs::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.file("out")).unwrap();
    let output = repo.casemap(&["init", "--flows", "out/x"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--flows must stay inside the repository")
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(!repo.file(".agents").exists());
}

#[cfg(unix)]
#[test]
fn init_accepts_catalog_through_inside_symlink() {
    let repo = init_repo();
    fs::create_dir(repo.file("real-docs")).unwrap();
    std::os::unix::fs::symlink("real-docs", repo.file("docs")).unwrap();
    stdout(&repo.casemap(&["init"]));
    assert!(repo.file("real-docs/flows/.gitkeep").is_file());
}

#[test]
fn init_rejects_repo_root_as_catalog() {
    for catalog in [".", "a/.."] {
        let repo = init_repo();
        let output = repo.casemap(&["init", "--flows", catalog]);
        assert_eq!(output.status.code(), Some(1), "{catalog}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--flows must name a directory inside the repository")
        );
        assert!(!repo.file(".gitkeep").exists());
        assert!(!repo.file(".agents").exists());
    }
}

#[cfg(unix)]
#[test]
fn init_ignores_planted_temp_symlink() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    let outside = assert_fs::TempDir::new().unwrap();
    let outside_file = outside.path().join("untouched");
    fs::write(&outside_file, "keep outside").unwrap();
    let skill_path = repo.file(&skill_paths()[0]);
    fs::write(&skill_path, "old skill").unwrap();
    let temporary = skill_path.with_file_name("SKILL.md.casemap-tmp");
    std::os::unix::fs::symlink(&outside_file, &temporary).unwrap();
    let output = repo.casemap(&["init", "--force"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&outside_file).unwrap(), "keep outside");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(&skill_path).unwrap(),
        skill::render(skill::DEFAULT_CATALOG)
    );
    assert!(fs::symlink_metadata(temporary).is_err());
}

#[cfg(unix)]
#[test]
fn init_force_keeps_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    let skill_path = repo.file(&skill_paths()[0]);
    fs::write(&skill_path, "old skill").unwrap();
    fs::set_permissions(&skill_path, fs::Permissions::from_mode(0o640)).unwrap();
    stdout(&repo.casemap(&["init", "--force"]));
    assert_eq!(
        fs::metadata(&skill_path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::read_to_string(skill_path).unwrap(),
        skill::render(skill::DEFAULT_CATALOG)
    );
}

#[cfg(unix)]
#[test]
fn init_rejects_dangling_symlink_catalog() {
    let repo = init_repo();
    std::os::unix::fs::symlink("missing", repo.file("out")).unwrap();
    let output = repo.casemap(&["init", "--flows", "out/flows"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--flows must stay inside the repository")
    );
    assert!(!repo.file(".agents").exists());
}

#[test]
fn check_reports_anchor_no_match() {
    let repo = common::anchored_catalog();
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("src/pay.rs", "src/missing.rs"),
    )
    .unwrap();
    let output = repo.casemap(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("[anchor-no-match]"));
}

#[test]
fn diff_reports_stale_flow_when_anchored_code_changes() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "fn pay() { changed(); }\n").unwrap();
    let text = stdout(&repo.casemap(&["diff"]));
    assert!(text.starts_with("no flow changes since HEAD ("));
    assert!(text.contains("possibly stale: 1 flow whose anchored code changed but the flow did not\n  Pay for an order (pay.md)\n    pay: src/pay.rs\n"), "{text}");
}

#[test]
fn diff_does_not_flag_a_touched_flow() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "changed\n").unwrap();
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("sees the result", "gets the result"),
    )
    .unwrap();
    assert!(!stdout(&repo.casemap(&["diff", "--strict"])).contains("possibly stale:"));
}

#[test]
fn diff_strict_fails_only_on_stale_or_broken() {
    let repo = common::anchored_catalog();
    assert_eq!(repo.casemap(&["diff", "--strict"]).status.code(), Some(0));
    fs::write(repo.file("src/pay.rs"), "changed\n").unwrap();
    assert_eq!(repo.casemap(&["diff"]).status.code(), Some(0));
    assert_eq!(repo.casemap(&["diff", "--strict"]).status.code(), Some(1));
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("pay[Pay]", "pay[Make payment]"),
    )
    .unwrap();
    assert_eq!(repo.casemap(&["diff", "--strict"]).status.code(), Some(0));
    fs::write(
        repo.flow_path("pay.md"),
        "```mermaid\nflowchart TD\na -->\n```\n",
    )
    .unwrap();
    for arguments in [&["diff"][..], &["diff", "--strict"]] {
        let output = repo.casemap(arguments);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stdout).contains("parse error"));
    }
}

#[test]
fn diff_counts_untracked_and_renamed_files() {
    let repo = common::anchored_catalog();
    repo.git(&["mv", "src/pay.rs", "src/payment.rs"]);
    assert!(stdout(&repo.casemap(&["diff"])).contains("pay: src/pay.rs"));
    repo.git(&["reset", "--hard", "-q", "HEAD"]);
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("src/pay.rs", "src/**"),
    )
    .unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "anchor folder"]);
    fs::write(repo.file("src/new.rs"), "new\n").unwrap();
    assert!(stdout(&repo.casemap(&["diff"])).contains("pay: src/new.rs"));
}

#[test]
fn diff_without_anchors_runs_no_extra_git() {
    let repo = TempRepo::committed_before();
    let sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    let trace_path = repo.file("git-trace.log");
    let output = casemap_command(&repo)
        .arg("diff")
        .env("GIT_TRACE", &trace_path)
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert_eq!(
        stdout(&output),
        format!("no flow changes since HEAD ({})\n", &sha[..7])
    );
    let trace = fs::read_to_string(trace_path).unwrap();
    assert!(trace.contains("merge-base"), "{trace}");
    assert!(!trace.contains("ls-files --others"), "{trace}");
    assert!(!trace.contains("diff --no-renames"), "{trace}");
}

#[test]
fn which_lists_anchored_steps() {
    let repo = common::anchored_catalog();
    assert_eq!(
        stdout(&repo.casemap(&["which", "src/pay.rs"])),
        "src/pay.rs\n  pay.md  Pay for an order  (steps: pay)\n"
    );
}

#[test]
fn which_reports_no_match() {
    let repo = common::anchored_catalog();
    assert_eq!(
        stdout(&repo.casemap(&["which", "src/other.rs"])),
        "src/other.rs: no anchored flow\n"
    );
}

#[test]
fn which_accepts_absolute_paths() {
    let repo = common::anchored_catalog();
    let output = repo.casemap(&["which", repo.file("src/pay.rs").to_str().unwrap()]);
    assert_eq!(
        stdout(&output),
        "src/pay.rs\n  pay.md  Pay for an order  (steps: pay)\n"
    );
}

#[test]
fn which_rejects_paths_outside_the_repo() {
    let repo = common::anchored_catalog();
    for path in ["../outside.rs", "src/../../outside.rs"] {
        let output = repo.casemap(&["which", path]);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains(&format!("{path} is outside the repository"))
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn which_expands_directories() {
    let repo = common::anchored_catalog();
    assert_eq!(
        stdout(&repo.casemap(&["which", "src"])),
        "src\n  pay.md  Pay for an order  (steps: pay)\n"
    );
}

#[test]
fn which_json_shape() {
    let repo = common::anchored_catalog();
    let text = stdout(&repo.casemap(&["which", "src/pay.rs", "--format", "json"]));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"version":1,"paths":[{"path":"src/pay.rs","flows":[{"name":"pay","path":"docs/flows/pay.md","title":"Pay for an order","steps":["pay"],"whole_flow":false}]}]})
    );
}

#[test]
fn which_merges_targets_ignores_unknown_steps_and_accepts_deleted_paths() {
    let repo = common::anchored_catalog();
    let source = common::append_step(
        common::ANCHORED_PAY,
        "%% @code * src\n%% @code pay src/**\n%% @code missing src/pay.rs\n",
    );
    fs::write(repo.flow_path("pay.md"), source).unwrap();
    fs::remove_file(repo.file("src/pay.rs")).unwrap();
    assert_eq!(
        stdout(&repo.casemap(&["which", "src/pay.rs", "src/other.rs"])),
        "src/pay.rs\n  pay.md  Pay for an order  (whole flow; steps: pay)\n\nsrc/other.rs\n  pay.md  Pay for an order  (whole flow; steps: pay)\n"
    );
}

#[test]
fn flows_name_prints_anchor_footer() {
    let repo = common::anchored_catalog();
    let source = common::append_step(
        common::ANCHORED_PAY,
        "%% @code * src\n%% @code pay src/other.rs\n",
    );
    fs::write(repo.flow_path("pay.md"), source).unwrap();
    assert!(
        stdout(&repo.casemap(&["flows", "pay"]))
            .ends_with("\nCode:\n  pay: src/pay.rs, src/other.rs\n  whole flow: src\n")
    );
}

#[test]
fn anchor_commands_use_the_catalog_repository_from_another_cwd() {
    let repo = common::anchored_catalog();
    let outside = TempRepo::new();
    let catalog = repo.file("docs/flows");
    let catalog = catalog.to_str().unwrap();
    let output = outside.casemap(&["check", "--strict", "--flows", catalog]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let output = outside.casemap(&[
        "which",
        repo.file("src/pay.rs").to_str().unwrap(),
        "--flows",
        catalog,
    ]);
    assert!(stdout(&output).contains("steps: pay"));
    fs::write(repo.file("src/pay.rs"), "changed").unwrap();
    assert!(stdout(&outside.casemap(&["diff", "--flows", catalog])).contains("pay: src/pay.rs"));
}

#[test]
fn check_and_which_work_without_git() {
    let repo = common::anchored_catalog();
    fs::remove_dir_all(repo.file(".git")).unwrap();
    for arguments in [&["check", "--strict"][..], &["which", "src/pay.rs"]] {
        let output = casemap_command(&repo)
            .args(arguments)
            .env("PATH", repo.file("no-git"))
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn which_expands_the_repository_root() {
    let repo = common::anchored_catalog();
    assert_eq!(
        stdout(&repo.casemap(&["which", "."])),
        ".\n  pay.md  Pay for an order  (steps: pay)\n"
    );
}

#[cfg(unix)]
#[test]
fn which_directory_arguments_are_literal() {
    let repo = common::anchored_catalog();
    fs::create_dir(repo.file("src/a*")).unwrap();
    fs::write(repo.file("src/a*/file.rs"), "").unwrap();
    fs::create_dir(repo.file("src/abc")).unwrap();
    fs::write(repo.file("src/abc/file.rs"), "").unwrap();
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("src/pay.rs", "src/abc"),
    )
    .unwrap();
    assert_eq!(
        stdout(&repo.casemap(&["which", "src/a*"])),
        "src/a*: no anchored flow\n"
    );
}

#[cfg(unix)]
#[test]
fn which_resolves_symlink_paths_and_rejects_symlink_escapes() {
    let repo = common::anchored_catalog();
    std::os::unix::fs::symlink("src", repo.file("alias")).unwrap();
    assert!(stdout(&repo.casemap(&["which", "alias/pay.rs"])).starts_with("src/pay.rs\n"));
    let outside = assert_fs::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.file("outside")).unwrap();
    let output = repo.casemap(&["which", "outside/deleted.rs"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("outside/deleted.rs is outside the repository")
    );
}

#[test]
fn check_builds_the_file_index_lazily_and_propagates_index_errors() {
    let repo = common::anchored_catalog();
    fs::write(repo.file(".git/config"), "[broken\n").unwrap();
    let output = repo.casemap(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("casemap: git "));
    assert!(output.stdout.is_empty());
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("%% @code pay src/pay.rs\n", ""),
    )
    .unwrap();
    assert_eq!(repo.casemap(&["check", "--strict"]).status.code(), Some(0));
}

#[test]
fn anchors_do_not_change_rendering() {
    let repo = common::anchored_catalog();
    let without_anchors = common::ANCHORED_PAY.replace("%% @code pay src/pay.rs\n", "");
    fs::write(repo.file("baseline.md"), &without_anchors).unwrap();
    let block = casemap::markdown::mermaid_blocks(&without_anchors).remove(0);
    fs::write(repo.file("baseline.mmd"), &block.source).unwrap();
    fs::write(
        repo.file("anchored.mmd"),
        format!("{}%% @code * src\n", block.source),
    )
    .unwrap();
    for direction in ["TD", "BT", "LR", "RL"] {
        for mode in ["--no-color", "--color", "--ascii"] {
            for (baseline, anchored) in [
                ("baseline.md", "docs/flows/pay.md"),
                ("baseline.mmd", "anchored.mmd"),
            ] {
                assert_eq!(
                    stdout(&repo.casemap(&[baseline, "--dir", direction, mode])),
                    stdout(&repo.casemap(&[anchored, "--dir", direction, mode]))
                );
            }
        }
    }
}

#[test]
fn stale_counts_valid_patterns_on_a_line_with_an_invalid_one() {
    let repo = common::anchored_catalog();
    fs::write(
        repo.flow_path("pay.md"),
        common::ANCHORED_PAY.replace("src/pay.rs", "src/pay.rs ../bad"),
    )
    .unwrap();
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "mixed patterns"]);
    fs::write(repo.file("src/pay.rs"), "fn changed() {}\n").unwrap();
    assert!(stdout(&repo.casemap(&["diff"])).contains("pay: src/pay.rs"));
    assert!(stdout(&repo.casemap(&["which", "src/pay.rs"])).contains("(steps: pay)"));
    let output = repo.casemap(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("[anchor-syntax]"), "{text}");
    assert!(text.contains("1 error"), "{text}");
    assert!(!text.contains("[anchor-no-match]"), "{text}");
    assert!(!text.contains("[anchor-unknown-step]"), "{text}");
}

#[test]
fn which_surfaces_git_errors() {
    let repo = common::anchored_catalog();
    fs::write(repo.file(".git/config"), "[broken\n").unwrap();
    let output = repo.casemap(&["which", "src/pay.rs"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("casemap: git"));
}

#[test]
fn check_and_which_work_outside_a_repository() {
    let repo = common::anchored_catalog();
    fs::remove_dir_all(repo.file(".git")).unwrap();
    for arguments in [&["check", "--strict"][..], &["which", "src/pay.rs"]] {
        let output = casemap_command(&repo)
            .args(arguments)
            .env("GIT_CEILING_DIRECTORIES", repo.path().parent().unwrap())
            .current_dir(repo.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn check_without_anchors_ignores_broken_git_config() {
    let repo = TempRepo::new();
    write_valid_flow(&repo, "checkout.md");
    repo.git(&["init", "-q"]);
    fs::write(repo.file(".git/config"), "[broken").unwrap();
    assert!(repo.casemap(&["check"]).status.success());
}

#[cfg(feature = "tui")]
#[test]
fn browse_requires_a_terminal() {
    let repo = TempRepo::committed_before();
    let output = repo.casemap(&["browse"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("casemap browse needs a terminal"));
}

#[cfg(feature = "tui")]
#[test]
fn browse_missing_catalog_fails_like_flows() {
    let repo = TempRepo::new();
    fs::remove_dir_all(repo.file(FLOWS_DIR)).unwrap();
    let browse = repo.casemap(&["browse"]);
    let flows = repo.casemap(&["flows"]);
    assert_eq!(browse.status.code(), Some(1));
    assert!(!browse.stderr.is_empty());
    assert_eq!(browse.stderr, flows.stderr);
}

#[cfg(feature = "tui")]
#[test]
fn browse_invalid_base_fails_like_diff() {
    let repo = TempRepo::committed_before();
    let browse = repo.casemap(&["browse", "--base", "missing-ref"]);
    let diff = repo.casemap(&["diff", "missing-ref"]);
    assert_eq!(browse.status.code(), Some(1));
    assert!(!browse.stderr.is_empty());
    assert_eq!(browse.stderr, diff.stderr);
}

#[test]
fn bare_casemap_with_piped_stdin_still_renders() {
    let repo = TempRepo::new();
    casemap_command(&repo)
        .current_dir(repo.path())
        .write_stdin("graph TD\na[First] --> b[Second]\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("First"));
}

#[cfg(not(feature = "tui"))]
#[test]
fn browse_without_tui_feature_errors() {
    let repo = TempRepo::new();
    let output = repo.casemap(&["browse"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("casemap was built without the tui feature")
    );
}

#[test]
fn bare_casemap_without_catalog_and_piped_stdin_still_renders() {
    let repo = TempRepo::new();
    std::fs::remove_dir(repo.file("docs/flows")).unwrap();
    casemap_command(&repo)
        .current_dir(repo.path())
        .write_stdin("graph TD\na[First] --> b[Second]\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("First"));
}

#[test]
fn init_agent_flag_needs_a_terminal() {
    let repo = TempRepo::new();
    std::fs::remove_dir(repo.file("docs/flows")).unwrap();
    casemap_command(&repo)
        .current_dir(repo.path())
        .args(["init", "--agent", "claude"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("needs a terminal"));
    assert!(!repo.file("docs/flows").exists());
}

const CI_WORKFLOW: &str = ".github/workflows/casemap.yml";

#[test]
fn init_ci_writes_the_workflow() {
    let repo = init_repo();
    let output = stdout(&repo.casemap(&["init", "--ci"]));
    let workflow = fs::read_to_string(repo.file(CI_WORKFLOW)).unwrap();
    assert!(output.contains("installed .github/workflows/casemap.yml (GitHub pull requests)"));
    assert!(workflow.contains("pull_request:"));
    assert!(workflow.contains("pull-requests: write"));
    assert!(workflow.contains("fetch-depth: 0"));
    assert!(workflow.contains("uses: Ross-cripto/casemap@v0"));
    assert!(workflow.contains("flows: 'docs/flows'"));
    assert_eq!(installed_skills(&repo).len(), 2);
}

#[test]
fn init_ci_is_idempotent() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init", "--ci"]));
    let before = fs::read(repo.file(CI_WORKFLOW)).unwrap();
    let output = stdout(&repo.casemap(&["init", "--ci"]));
    assert!(output.contains("up to date .github/workflows/casemap.yml"));
    assert_eq!(fs::read(repo.file(CI_WORKFLOW)).unwrap(), before);
}

#[test]
fn init_ci_keeps_a_differing_workflow_without_force() {
    let repo = init_repo();
    fs::create_dir_all(repo.file(".github/workflows")).unwrap();
    fs::write(repo.file(CI_WORKFLOW), "name: custom\n").unwrap();
    let output = stdout(&repo.casemap(&["init", "--ci"]));
    assert!(output.contains("kept .github/workflows/casemap.yml (differs from this casemap version; use --force to update)"));
    assert_eq!(
        fs::read_to_string(repo.file(CI_WORKFLOW)).unwrap(),
        "name: custom\n"
    );
}

#[test]
fn init_ci_force_updates_only_the_workflow() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init", "--ci"]));
    let expected = fs::read(repo.file(CI_WORKFLOW)).unwrap();
    fs::write(repo.file(CI_WORKFLOW), "name: custom\n").unwrap();
    fs::write(repo.file(".github/workflows/other.yml"), "name: other\n").unwrap();
    fs::write(repo.flow_path("custom.md"), "my flow\n").unwrap();
    let skills = installed_skills(&repo);
    let output = stdout(&repo.casemap(&["init", "--ci", "--force"]));
    assert!(output.contains("updated .github/workflows/casemap.yml"));
    assert_eq!(fs::read(repo.file(CI_WORKFLOW)).unwrap(), expected);
    assert_eq!(
        fs::read_to_string(repo.file(".github/workflows/other.yml")).unwrap(),
        "name: other\n"
    );
    assert_eq!(
        fs::read_to_string(repo.flow_path("custom.md")).unwrap(),
        "my flow\n"
    );
    assert_eq!(installed_skills(&repo), skills);
}

#[test]
fn init_ci_uses_root_relative_catalog_from_a_subdirectory() {
    let repo = init_repo();
    fs::create_dir(repo.file("app")).unwrap();
    let output = casemap_command(&repo)
        .current_dir(repo.file("app"))
        .args(["init", "--ci", "--flows", "flows"])
        .output()
        .unwrap();
    stdout(&output);
    assert!(
        fs::read_to_string(repo.file(CI_WORKFLOW))
            .unwrap()
            .contains("flows: 'app/flows'")
    );
    assert!(!repo.file("app/.github").exists());
}

#[test]
fn init_without_ci_does_not_write_a_workflow() {
    let repo = init_repo();
    stdout(&repo.casemap(&["init"]));
    assert!(!repo.file(CI_WORKFLOW).exists());
}

#[test]
fn diff_markdown_quiet_has_no_colour() {
    let repo = common::anchored_catalog();
    let quiet = stdout(&repo.casemap(&["diff", "--format", "markdown", "--color"]));
    assert!(quiet.contains(flows::QUIET_MARKER));
    assert!(!quiet.contains('\x1b'));
}

#[test]
fn diff_markdown_ignores_color() {
    let repo = common::anchored_catalog();
    fs::write(
        repo.flow_path("new.md"),
        "# New\n\nDescription.\n\n```mermaid\ngraph TD\na[Added] --> b[Done]\n```\n",
    )
    .unwrap();
    let output = stdout(&repo.casemap(&["diff", "--format", "markdown", "--color"]));
    assert!(output.contains("```mermaid"));
    assert!(output.contains("class n0,n1 added"));
    assert!(!output.contains('\x1b'));
}

#[test]
fn diff_markdown_stale_fails_only_with_strict() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "fn pay() { changed(); }\n").unwrap();
    let stale = stdout(&repo.casemap(&["diff", "--format", "markdown"]));
    assert!(stale.contains("#### ⚠️ Possibly stale"));
    assert_eq!(
        repo.casemap(&["diff", "--format", "markdown", "--strict"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn diff_markdown_broken_flow_fails() {
    let repo = common::anchored_catalog();
    fs::write(
        repo.flow_path("broken.md"),
        "# Broken\n\nDescription.\n\n```mermaid\nflowchart TD\na -->\n```\n",
    )
    .unwrap();
    let broken = repo.casemap(&["diff", "--format", "markdown"]);
    assert_eq!(broken.status.code(), Some(1));
    let body = String::from_utf8(broken.stdout).unwrap();
    assert!(body.contains("(parse error)"));
    assert!(body.contains("[parse-error]"));
}

#[test]
fn diff_markdown_bad_base_prints_no_comment() {
    let repo = common::anchored_catalog();
    let output = repo.casemap(&["diff", "invalid-ref", "--format", "markdown"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn diff_markdown_fails_check_errors_even_without_diagram_changes() {
    let repo = common::anchored_catalog();
    let path = repo.flow_path("pay.md");
    let original = fs::read_to_string(&path).unwrap();
    let without_title = original.lines().skip(1).collect::<Vec<_>>().join("\n");
    fs::write(path, without_title).unwrap();
    let output = repo.casemap(&["diff", "--format", "markdown"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("[missing-title]")
    );
    assert_eq!(repo.casemap(&["diff"]).status.code(), Some(0));
}

#[test]
fn diff_markdown_warnings_do_not_fail_even_with_strict() {
    let repo = common::anchored_catalog();
    fs::write(repo.flow_path("warning.mmd"), "flowchart TD\na[A]").unwrap();
    let output = stdout(&repo.casemap(&["diff", "--format", "markdown", "--strict"]));
    assert!(output.contains("[mmd-flow]"));
    assert!(!output.contains(flows::QUIET_MARKER));
}

#[test]
fn diff_markdown_draws_old_diagram_when_block_is_deleted() {
    let repo = TempRepo::committed_before();
    fs::write(
        repo.flow_path("login.md"),
        "# Login\n\nA person signs in.\n",
    )
    .unwrap();
    let output = repo.casemap(&["diff", "--format", "markdown"]);
    assert_eq!(output.status.code(), Some(1));
    let body = String::from_utf8(output.stdout).unwrap();
    assert!(body.contains("#### − Login · `docs/flows/login.md` (removed)"));
    assert!(body.contains("class "));
    assert!(body.contains(" removed"));
    assert!(body.contains("[missing-diagram]"));
}

#[cfg(unix)]
#[test]
fn init_ci_rejects_workflows_dir_symlinked_outside() {
    for component in [".github", ".github/workflows"] {
        let repo = init_repo();
        let outside = assert_fs::TempDir::new().unwrap();
        if component == ".github/workflows" {
            fs::create_dir(repo.file(".github")).unwrap();
        }
        std::os::unix::fs::symlink(outside.path(), repo.file(component)).unwrap();
        let output = repo.casemap(&["init", "--ci", "--force"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("error:"));
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}

#[cfg(unix)]
#[test]
fn init_ci_reports_dangling_workflow_symlink() {
    let repo = init_repo();
    fs::create_dir_all(repo.file(".github/workflows")).unwrap();
    std::os::unix::fs::symlink(repo.file("missing.yml"), repo.file(CI_WORKFLOW)).unwrap();
    let output = repo.casemap(&["init", "--ci", "--force"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("symlink to a missing path"));
    assert!(!repo.file("missing.yml").exists());
}

#[cfg(unix)]
#[test]
fn init_ci_writes_through_a_workflow_file_symlink_inside_the_repo() {
    let repo = init_repo();
    fs::create_dir_all(repo.file(".github/workflows")).unwrap();
    fs::write(repo.file("shared-workflow.yml"), "name: custom\n").unwrap();
    std::os::unix::fs::symlink(repo.file("shared-workflow.yml"), repo.file(CI_WORKFLOW)).unwrap();
    stdout(&repo.casemap(&["init", "--ci", "--force"]));
    assert!(repo.file(CI_WORKFLOW).is_symlink());
    assert!(
        fs::read_to_string(repo.file("shared-workflow.yml"))
            .unwrap()
            .contains("flows: 'docs/flows'")
    );
}

#[test]
fn init_ci_warns_when_workflow_is_gitignored() {
    let repo = init_repo();
    fs::write(repo.file(".gitignore"), ".github/\n").unwrap();
    let output = stdout(&repo.casemap(&["init", "--ci"]));
    assert!(output.contains(
        "warning: .github/workflows/casemap.yml is gitignored; teammates will not get the workflow"
    ));
}
