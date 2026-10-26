#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub const FLOWS_DIR: &str = "docs/flows";
pub const BASE_BRANCH: &str = "main";
const FEATURE_BRANCH: &str = "feature";
const LOGOUT_STEP: &str = "    home --> logout[Sign out]\n";
const GIT_LOCATION_VARS: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

pub fn isolate_git(command: &mut Command, config: &Path) {
    for variable in GIT_LOCATION_VARS {
        command.env_remove(variable);
    }
    command
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

pub fn git_command(config: &Path) -> Command {
    let mut command = Command::new("git");
    isolate_git(&mut command, config);
    command
        .env("GIT_AUTHOR_NAME", "casemap")
        .env("GIT_AUTHOR_EMAIL", "casemap@example.com")
        .env("GIT_COMMITTER_NAME", "casemap")
        .env("GIT_COMMITTER_EMAIL", "casemap@example.com")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+00:00")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+00:00")
        .args(["-c", "commit.gpgsign=false"]);
    command
}

#[cfg(feature = "cli")]
pub fn casemap_command(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_casemap"));
    isolate_git(&mut command, config);
    command.env_remove("NO_COLOR");
    command
}

pub struct TempRepo {
    directory: assert_fs::TempDir,
    path: PathBuf,
}

impl TempRepo {
    pub fn new() -> Self {
        let directory = assert_fs::TempDir::new().unwrap();
        fs::create_dir(directory.path().join("repo")).unwrap();
        let path = directory.path().join("repo");
        let repo = Self { directory, path };
        fs::write(repo.config_path(), "").unwrap();
        fs::create_dir_all(repo.file(FLOWS_DIR)).unwrap();
        repo
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn config_path(&self) -> PathBuf {
        self.directory.path().join("gitconfig")
    }

    pub fn file(&self, relative: &str) -> PathBuf {
        self.path().join(relative)
    }

    pub fn flow_path(&self, name: &str) -> PathBuf {
        self.file(FLOWS_DIR).join(name)
    }

    pub fn git(&self, args: &[&str]) {
        self.git_stdout(args);
    }

    pub fn git_stdout(&self, args: &[&str]) -> String {
        let output = git_command(&self.config_path())
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

    pub fn add_fixture(&self, side: &str, name: &str) {
        fs::copy(
            Path::new("tests/fixtures/flows").join(side).join(name),
            self.flow_path(name),
        )
        .unwrap();
    }

    pub fn copy_catalog(&self, side: &str) {
        for entry in fs::read_dir(format!("tests/fixtures/flows/{side}")).unwrap() {
            let name = entry.unwrap().file_name();
            self.add_fixture(side, name.to_str().unwrap());
        }
    }

    pub fn committed_before() -> Self {
        let repo = Self::new();
        repo.copy_catalog("before");
        repo.git(&["init", "-q", "-b", BASE_BRANCH]);
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", "base flows"]);
        repo
    }

    #[cfg(feature = "cli")]
    pub fn casemap(&self, args: &[&str]) -> Output {
        casemap_command(&self.config_path())
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap()
    }
}

pub fn append_step(markdown: &str, step: &str) -> String {
    let closing_fence = markdown.rfind("```").expect("flow has a closing fence");
    let mut updated = markdown.to_string();
    updated.insert_str(closing_fence, step);
    updated
}

pub fn divergent_catalog() -> TempRepo {
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

#[cfg(feature = "cli")]
impl TempRepo {
    pub fn casemap_stdin(&self, args: &[&str], input: &str) -> Output {
        let mut child = casemap_command(&self.config_path())
            .args(args)
            .current_dir(self.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

pub struct CliRun {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl From<Output> for CliRun {
    fn from(output: Output) -> Self {
        Self {
            status: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

#[cfg(feature = "cli")]
pub fn run_casemap(args: &[&str], cwd: &Path, env: &[(&str, &str)], stdin: Option<&str>) -> CliRun {
    let mut command = Command::new(env!("CARGO_BIN_EXE_casemap"));
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("NO_COLOR")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Some(stdin) = stdin {
        input.write_all(stdin.as_bytes()).unwrap();
    }
    drop(input);
    child.wait_with_output().unwrap().into()
}

pub fn transcript(args: &str, run: &CliRun) -> String {
    format!(
        "$ casemap {args}\n[exit {}]\n--- stdout\n{}--- stderr\n{}[end]\n",
        run.status, run.stdout, run.stderr
    )
    .replace('\x1b', "␛")
}

pub const ANCHORED_PAY: &str = "# Pay for an order\n\nA customer pays and sees the result.\n\n```mermaid\nflowchart TD\nactor([Customer]) --> pay[Pay]\npay --> done[Show result]\n%% @code pay src/pay.rs\n```\n";

pub fn anchored_catalog() -> TempRepo {
    let repo = TempRepo::new();
    fs::create_dir(repo.file("src")).unwrap();
    fs::write(repo.file("src/pay.rs"), "fn pay() {}\n").unwrap();
    fs::write(repo.flow_path("pay.md"), ANCHORED_PAY).unwrap();
    repo.git(&["init", "-q", "-b", BASE_BRANCH]);
    repo.git(&["add", "."]);
    repo.git(&["commit", "-qm", "anchored flow"]);
    repo
}
