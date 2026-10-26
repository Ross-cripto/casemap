use std::ffi::OsStr;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anyhow::Context;
use casemap::flows;

const MAX_MENU_PROMPTS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CatalogState {
    Missing,
    Empty,
    HasFlows,
}

pub(crate) fn catalog_state(dir: &Path) -> CatalogState {
    if !dir.is_dir() {
        return CatalogState::Missing;
    }
    match flows::read_catalog(dir) {
        Ok(entries)
            if entries
                .iter()
                .any(|entry| matches!(entry, flows::Entry::Flow { .. })) =>
        {
            CatalogState::HasFlows
        }
        _ => CatalogState::Empty,
    }
}

#[derive(Debug)]
pub(crate) struct BareModeInputs {
    pub interactive: bool,
    pub catalog: CatalogState,
    pub watch: bool,
    pub has_file: bool,
    pub browser_available: bool,
}

pub(crate) fn is_interactive_session(
    stdin_tty: bool,
    stdout_tty: bool,
    ci: bool,
    term_dumb: bool,
) -> bool {
    stdin_tty && stdout_tty && !ci && !term_dumb
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BareMode {
    Browse,
    Onboard,
    ReadStdin,
}

pub(crate) fn bare_mode(inputs: &BareModeInputs) -> BareMode {
    if inputs.has_file || inputs.watch || !inputs.interactive {
        return BareMode::ReadStdin;
    }
    match inputs.catalog {
        CatalogState::HasFlows if inputs.browser_available => BareMode::Browse,
        CatalogState::HasFlows => BareMode::ReadStdin,
        CatalogState::Missing | CatalogState::Empty => BareMode::Onboard,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub(crate) const ALL: [Agent; 2] = [Agent::Claude, Agent::Codex];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    pub(crate) fn program(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

pub(crate) fn find_agent(
    agent: Agent,
    path_var: Option<&OsStr>,
    windows: bool,
    is_executable: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let program = if windows {
        format!("{}.exe", agent.program())
    } else {
        agent.program().to_string()
    };
    std::env::split_paths(path_var?)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(&program))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(crate) fn detect_agents() -> Vec<(Agent, PathBuf)> {
    let path = std::env::var_os("PATH");
    Agent::ALL
        .into_iter()
        .filter_map(|agent| {
            find_agent(agent, path.as_deref(), cfg!(windows), &is_executable)
                .map(|program| (agent, program))
        })
        .collect()
}

pub(crate) fn agent_prompt(catalog: &str) -> String {
    format!(
        "Use the casemap skill to document the use-case flows of this repository in {catalog}/. Follow its \"Document a repository\" steps, and finish when `casemap check --strict` passes."
    )
}

pub(crate) fn agent_command(program: &Path, prompt: &str, cwd: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .arg(prompt)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

#[derive(Debug)]
pub(crate) enum AgentExit {
    Success,
    Failed(String),
}

pub(crate) fn launch(mut command: Command) -> anyhow::Result<AgentExit> {
    let status = command.status()?;
    if status.success() {
        Ok(AgentExit::Success)
    } else {
        Ok(AgentExit::Failed(match status.code() {
            Some(code) => format!("code {code}"),
            None => status.to_string(),
        }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuItem {
    Agent(Agent),
    Steps,
}

impl MenuItem {
    fn label(self) -> String {
        match self {
            Self::Agent(agent) => agent_menu_label(agent),
            Self::Steps => "Just show me the steps".to_string(),
        }
    }
}

fn agent_menu_label(agent: Agent) -> String {
    format!("Create it with {}", agent.label())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice<T = MenuItem> {
    Item(T),
    Quit,
}

pub(crate) fn menu_items(agents: &[Agent], with_steps: bool) -> Vec<MenuItem> {
    let mut items: Vec<_> = agents.iter().copied().map(MenuItem::Agent).collect();
    if with_steps {
        items.push(MenuItem::Steps);
    }
    items
}

pub(crate) fn parse_choice<T: Copy>(input: &str, items: &[T]) -> Option<Choice<T>> {
    let input = input.trim();
    if matches!(input, "" | "q" | "Q") {
        return Some(Choice::Quit);
    }
    items
        .iter()
        .enumerate()
        .find_map(|(index, item)| (input == (index + 1).to_string()).then_some(Choice::Item(*item)))
}

pub(crate) trait MenuInput {
    fn read_menu_line(&mut self, line: &mut String) -> std::io::Result<usize>;
}

impl<T: BufRead> MenuInput for T {
    fn read_menu_line(&mut self, line: &mut String) -> std::io::Result<usize> {
        BufRead::read_line(self, line)
    }
}

#[derive(Debug)]
pub(crate) enum Outcome {
    Browse(PathBuf),
    Exit(ExitCode),
}

pub(crate) struct Session<'a> {
    pub cwd: &'a Path,
    pub dir: &'a Path,
    pub state: CatalogState,
    pub agents: Vec<(Agent, PathBuf)>,
    pub input: &'a mut dyn MenuInput,
    pub output: &'a mut dyn Write,
    pub launch: &'a mut dyn FnMut(Command) -> anyhow::Result<AgentExit>,
}

impl Session<'_> {
    fn agent_choices(&self) -> Vec<Agent> {
        self.agents.iter().map(|(agent, _)| *agent).collect()
    }
}

fn print_steps(output: &mut dyn Write, display: &str) -> anyhow::Result<()> {
    writeln!(output, "To map this repository by hand:")?;
    writeln!(output, "  1. casemap init")?;
    writeln!(
        output,
        "  2. Ask your coding agent: \"{}\"",
        agent_prompt(display)
    )?;
    writeln!(output, "  3. casemap browse")?;
    Ok(())
}

fn choose<T: Copy>(
    session: &mut Session,
    items: &[T],
    label: fn(T) -> String,
    action: &str,
) -> anyhow::Result<Choice<T>> {
    writeln!(session.output)?;
    for (index, item) in items.iter().enumerate() {
        writeln!(session.output, "  {}  {}", index + 1, label(*item))?;
    }
    writeln!(session.output)?;
    for _ in 0..MAX_MENU_PROMPTS {
        write!(
            session.output,
            "Choose 1-{} (Enter to {action}): ",
            items.len()
        )?;
        session.output.flush()?;
        let mut input = String::new();
        session.input.read_menu_line(&mut input)?;
        if let Some(choice) = parse_choice(&input, items) {
            return Ok(choice);
        }
    }
    Ok(Choice::Quit)
}

fn agent_path(agents: &[(Agent, PathBuf)], agent: Agent) -> anyhow::Result<PathBuf> {
    agents
        .iter()
        .find(|(candidate, _)| *candidate == agent)
        .map(|(_, program)| program.clone())
        .with_context(|| format!("`{}` was not found on PATH", agent.program()))
}

pub(crate) fn preflight(
    agent: Option<Agent>,
    interactive: bool,
    agents: &[(Agent, PathBuf)],
) -> anyhow::Result<Option<(Agent, PathBuf)>> {
    let Some(agent) = agent else {
        return Ok(None);
    };
    if !interactive {
        anyhow::bail!("--agent needs a terminal (stdin and stdout)");
    }
    agent_path(agents, agent).map(|program| Some((agent, program)))
}

pub(crate) fn run(mut session: Session) -> anyhow::Result<Outcome> {
    if session.state == CatalogState::HasFlows {
        return Ok(Outcome::Browse(session.dir.to_path_buf()));
    }
    let (_, display) = crate::catalog_display(session.cwd, session.dir)?;
    if session.state == CatalogState::Empty {
        writeln!(session.output, "The use-case map in {display}/ is empty.")?;
    } else {
        writeln!(session.output, "No use-case map in this repository yet.")?;
    }
    writeln!(
        session.output,
        "casemap keeps one Mermaid flow per use case in {display}/, written by your coding agent."
    )?;
    if session.agents.is_empty() {
        print_steps(session.output, &display)?;
        return Ok(Outcome::Exit(ExitCode::SUCCESS));
    }
    let agents = session.agent_choices();
    let items = menu_items(&agents, true);
    match choose(&mut session, &items, MenuItem::label, "quit")? {
        Choice::Item(MenuItem::Agent(agent)) => {
            let program = agent_path(&session.agents, agent)?;
            let initialized = crate::init_core(session.cwd, session.dir, false)?;
            if initialized.failed {
                return Ok(Outcome::Exit(ExitCode::FAILURE));
            }
            launch_and_finish(
                &mut session,
                agent,
                &program,
                &initialized.root,
                &initialized.catalog,
                &initialized.display,
            )
        }
        Choice::Item(MenuItem::Steps) => {
            print_steps(session.output, &display)?;
            Ok(Outcome::Exit(ExitCode::SUCCESS))
        }
        Choice::Quit => Ok(Outcome::Exit(ExitCode::SUCCESS)),
    }
}

pub(crate) fn offer(
    session: &mut Session,
    root: &Path,
    catalog: &Path,
    display: &str,
) -> anyhow::Result<Outcome> {
    writeln!(session.output, "\nOpen a coding agent now to write them?")?;
    let agents = session.agent_choices();
    match choose(session, &agents, agent_menu_label, "skip")? {
        Choice::Item(agent) => {
            let program = agent_path(&session.agents, agent)?;
            launch_and_finish(session, agent, &program, root, catalog, display)
        }
        Choice::Quit => Ok(Outcome::Exit(ExitCode::SUCCESS)),
    }
}

pub(crate) fn launch_and_finish(
    session: &mut Session,
    agent: Agent,
    program: &Path,
    root: &Path,
    catalog: &Path,
    display: &str,
) -> anyhow::Result<Outcome> {
    let prompt = agent_prompt(display);
    let label = agent.label();
    writeln!(
        session.output,
        "\nOpening {label} with: \"{prompt}\"\nExit {label} when it is done to come back to casemap."
    )?;
    session.output.flush()?;
    let exit = (session.launch)(agent_command(program, &prompt, root))
        .with_context(|| format!("starting {}", program.display()))?;
    if let AgentExit::Failed(status) = exit {
        writeln!(session.output, "{label} exited with {status}")?;
    }
    let report = crate::check_report(catalog).context("checking the catalog")?;
    crate::write_check_text(session.output, Path::new(display), &report, false)?;
    if report.flows >= 1 {
        return Ok(Outcome::Browse(session.dir.to_path_buf()));
    }
    writeln!(
        session.output,
        "No flows were written. Run casemap again to retry, or ask your agent: \"{prompt}\""
    )?;
    Ok(Outcome::Exit(ExitCode::FAILURE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;

    #[test]
    fn bare_mode_table() {
        struct Case {
            name: &'static str,
            stdin_tty: bool,
            stdout_tty: bool,
            ci: bool,
            term_dumb: bool,
            catalog: CatalogState,
            watch: bool,
            has_file: bool,
            browser_available: bool,
            expected: BareMode,
        }
        let interactive = Case {
            name: "flows with browser",
            stdin_tty: true,
            stdout_tty: true,
            ci: false,
            term_dumb: false,
            catalog: CatalogState::HasFlows,
            watch: false,
            has_file: false,
            browser_available: true,
            expected: BareMode::Browse,
        };
        let cases = [
            Case {
                name: "missing catalog",
                catalog: CatalogState::Missing,
                expected: BareMode::Onboard,
                ..interactive
            },
            Case {
                name: "empty catalog",
                catalog: CatalogState::Empty,
                expected: BareMode::Onboard,
                ..interactive
            },
            Case {
                name: "piped stdin",
                stdin_tty: false,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            Case {
                name: "piped stdout",
                stdout_tty: false,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            Case {
                name: "CI set",
                ci: true,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            Case {
                name: "TERM=dumb",
                term_dumb: true,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            Case {
                name: "watch",
                watch: true,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            Case {
                name: "FILE argument",
                has_file: true,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            Case {
                name: "flows without browser",
                browser_available: false,
                expected: BareMode::ReadStdin,
                ..interactive
            },
            interactive,
        ];
        for case in cases {
            let inputs = BareModeInputs {
                interactive: is_interactive_session(
                    case.stdin_tty,
                    case.stdout_tty,
                    case.ci,
                    case.term_dumb,
                ),
                catalog: case.catalog,
                watch: case.watch,
                has_file: case.has_file,
                browser_available: case.browser_available,
            };
            assert_eq!(bare_mode(&inputs), case.expected, "{}", case.name);
        }
    }

    #[test]
    fn bare_mode_never_onboards_outside_an_interactive_session() {
        const BOOLEAN_COMBINATIONS: usize = 1 << 7;
        for bits in 0..BOOLEAN_COMBINATIONS {
            let stdin_tty = bits & 1 != 0;
            let stdout_tty = bits & 2 != 0;
            let ci = bits & 4 != 0;
            let term_dumb = bits & 8 != 0;
            let watch = bits & 16 != 0;
            let has_file = bits & 32 != 0;
            let browser_available = bits & 64 != 0;
            for catalog in [
                CatalogState::Missing,
                CatalogState::Empty,
                CatalogState::HasFlows,
            ] {
                let inputs = BareModeInputs {
                    interactive: is_interactive_session(stdin_tty, stdout_tty, ci, term_dumb),
                    catalog,
                    watch,
                    has_file,
                    browser_available,
                };
                let blocked = [!stdin_tty, !stdout_tty, ci, term_dumb, watch, has_file];
                let expected = if blocked.contains(&true) {
                    BareMode::ReadStdin
                } else if catalog != CatalogState::HasFlows {
                    BareMode::Onboard
                } else if browser_available {
                    BareMode::Browse
                } else {
                    BareMode::ReadStdin
                };
                assert_eq!(bare_mode(&inputs), expected, "bits={bits}, {inputs:?}");
            }
        }
    }

    #[test]
    fn catalog_state_reads_the_directory() {
        let directory = assert_fs::TempDir::new().unwrap();
        let catalog = directory.path().join("flows");
        assert_eq!(catalog_state(&catalog), CatalogState::Missing);
        fs::create_dir(&catalog).unwrap();
        fs::write(catalog.join(".gitkeep"), "").unwrap();
        fs::write(catalog.join("README.md"), "readme").unwrap();
        assert_eq!(catalog_state(&catalog), CatalogState::Empty);
        fs::write(catalog.join("a.md"), "flow").unwrap();
        assert_eq!(catalog_state(&catalog), CatalogState::HasFlows);
    }

    #[test]
    fn find_agent_on_unix_path() {
        // Built from temp_dir so the entries are absolute on Windows too.
        let first = std::env::temp_dir().join("a");
        let second = std::env::temp_dir().join("b");
        let path = std::env::join_paths([&first, &second]).unwrap();
        let claude = second.join("claude");
        let executable = |candidate: &Path| candidate == claude;
        assert_eq!(
            find_agent(Agent::Claude, Some(&path), false, &executable),
            Some(claude.clone())
        );
        assert_eq!(
            find_agent(Agent::Codex, Some(&path), false, &executable),
            None
        );
        assert_eq!(find_agent(Agent::Claude, None, false, &executable), None);
    }

    #[test]
    fn windows_detection_ignores_cmd_shims() {
        let directory = assert_fs::TempDir::new().unwrap();
        let path = std::env::join_paths([directory.path()]).unwrap();
        fs::write(directory.path().join("claude.cmd"), "").unwrap();
        fs::write(directory.path().join("codex.bat"), "").unwrap();
        assert_eq!(
            find_agent(Agent::Claude, Some(&path), true, &Path::is_file),
            None
        );
        assert_eq!(
            find_agent(Agent::Codex, Some(&path), true, &Path::is_file),
            None
        );
        let program = directory.path().join("claude.exe");
        fs::write(&program, "").unwrap();
        assert_eq!(
            find_agent(Agent::Claude, Some(&path), true, &Path::is_file),
            Some(program)
        );
    }

    #[test]
    fn agent_prompt_names_catalog_and_skill() {
        assert_eq!(
            agent_prompt("web/docs/flows"),
            "Use the casemap skill to document the use-case flows of this repository in web/docs/flows/. Follow its \"Document a repository\" steps, and finish when `casemap check --strict` passes."
        );
    }

    #[test]
    fn agent_command_passes_one_positional_prompt() {
        let command = agent_command(Path::new("/b/claude"), "prompt", Path::new("/repo"));
        assert_eq!(command.get_program(), "/b/claude");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["prompt"]);
        assert_eq!(command.get_current_dir(), Some(Path::new("/repo")));
    }

    #[test]
    fn parse_choice_cases() {
        let items = menu_items(&Agent::ALL, true);
        for (input, expected) in [
            ("1", Some(Choice::Item(MenuItem::Agent(Agent::Claude)))),
            (" 2\n", Some(Choice::Item(MenuItem::Agent(Agent::Codex)))),
            ("3", Some(Choice::Item(MenuItem::Steps))),
            ("", Some(Choice::Quit)),
            ("q", Some(Choice::Quit)),
            ("Q", Some(Choice::Quit)),
            ("9", None),
            ("0", None),
            ("01", None),
            ("+1", None),
            ("x", None),
        ] {
            assert_eq!(parse_choice(input, &items), expected, "{input:?}");
        }
    }

    fn run_menu(
        input: &str,
        agents: Vec<(Agent, PathBuf)>,
        state: CatalogState,
    ) -> (Outcome, String, assert_fs::TempDir) {
        let directory = assert_fs::TempDir::new().unwrap();
        let mut input = Cursor::new(input);
        let mut output = Vec::new();
        let mut launch = |_: Command| panic!("menu must not launch an agent");
        let outcome = run(Session {
            cwd: directory.path(),
            dir: Path::new("docs/flows"),
            state,
            agents,
            input: &mut input,
            output: &mut output,
            launch: &mut launch,
        })
        .unwrap();
        (outcome, String::from_utf8(output).unwrap(), directory)
    }

    const ISOLATED_TEST_ENV: &str = "CASEMAP_ONBOARD_GIT_TEST";
    const GIT_CEILING_ENV: &str = "GIT_CEILING_DIRECTORIES";
    const GIT_GLOBAL_CONFIG_ENV: &str = "GIT_CONFIG_GLOBAL";
    const GIT_NO_SYSTEM_CONFIG_ENV: &str = "GIT_CONFIG_NOSYSTEM";
    const GIT_REPOSITORY_ENV: &[&str] = &[
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ];
    const VALID_FLOW: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/flows/after/checkout.md"
    );

    fn in_isolated_git(test_path: &str, body: impl FnOnce()) {
        if std::env::var(ISOLATED_TEST_ENV).as_deref() == Ok(test_path) {
            body();
        } else {
            let mut command = Command::new(std::env::current_exe().unwrap());
            for variable in GIT_REPOSITORY_ENV {
                command.env_remove(variable);
            }
            let output = command
                .args(["--exact", test_path, "--nocapture"])
                .env(ISOLATED_TEST_ENV, test_path)
                .env(GIT_CEILING_ENV, std::env::temp_dir())
                .env(
                    GIT_GLOBAL_CONFIG_ENV,
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                )
                .env(GIT_NO_SYSTEM_CONFIG_ENV, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
        }
    }

    macro_rules! isolated_git_test {
        ($name:ident, $body:block) => {
            #[test]
            fn $name() {
                let module = module_path!().split_once("::").unwrap().1;
                in_isolated_git(&format!("{module}::{}", stringify!($name)), || $body);
            }
        };
    }

    isolated_git_test!(menu_lists_agents_then_steps, {
        assert_eq!(
            menu_items(&[Agent::Codex], true),
            [MenuItem::Agent(Agent::Codex), MenuItem::Steps]
        );
        assert_eq!(
            menu_items(&[Agent::Codex], false),
            [MenuItem::Agent(Agent::Codex)]
        );
        let (_, output, _) = run_menu(
            "2\n",
            vec![(Agent::Codex, "/x/codex".into())],
            CatalogState::Missing,
        );
        assert!(output.contains("1  Create it with Codex"));
        assert!(output.contains("2  Just show me the steps"));
        assert!(output.contains("To map this repository by hand:"));
    });

    isolated_git_test!(no_agents_prints_steps_without_menu, {
        let (outcome, output, directory) = run_menu("", vec![], CatalogState::Missing);
        assert!(matches!(outcome, Outcome::Exit(ExitCode::SUCCESS)));
        assert!(output.contains("To map this repository by hand:"));
        assert!(!output.contains("Choose"));
        assert!(!directory.path().join("docs").exists());
    });

    isolated_git_test!(invalid_choice_reprompts_then_quits, {
        let (outcome, output, directory) = run_menu(
            "x\ny\nz\nw\nv\n",
            vec![(Agent::Claude, "/x/claude".into())],
            CatalogState::Missing,
        );
        assert!(matches!(outcome, Outcome::Exit(ExitCode::SUCCESS)));
        assert_eq!(output.matches("Choose 1-2 (Enter to quit): ").count(), 5);
        assert!(!directory.path().join("docs").exists());
    });

    fn repository() -> assert_fs::TempDir {
        let directory = assert_fs::TempDir::new().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
        directory
    }

    /// The root as git reports it, which is where onboarding launches the agent. Unlike
    /// `canonicalize`, it has no `\\?\` prefix on Windows, so joins with '/' keep working.
    fn repository_root(directory: &assert_fs::TempDir) -> PathBuf {
        crate::catalog_display(directory.path(), Path::new("docs/flows"))
            .unwrap()
            .0
    }

    enum AgentWrites {
        ValidFlow,
        InvalidFlow,
        Nothing,
        RemoveCatalog,
    }

    fn assert_agent_command(command: &Command, root: &Path, display: &str) {
        assert_eq!(command.get_program(), "/x/claude");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new(&agent_prompt(display))]
        );
        assert_eq!(command.get_current_dir(), Some(root));
    }

    fn write_agent_flow(catalog: &Path, writes: AgentWrites) -> anyhow::Result<()> {
        match writes {
            AgentWrites::ValidFlow => {
                fs::copy(VALID_FLOW, catalog.join("checkout.md"))?;
            }
            AgentWrites::InvalidFlow => {
                fs::write(
                    catalog.join("checkout.md"),
                    "# Checkout\n\n```mermaid\nflowchart TD\n    start --> finish\n```\n",
                )?;
            }
            AgentWrites::Nothing => {}
            AgentWrites::RemoveCatalog => fs::remove_dir_all(catalog)?,
        }
        Ok(())
    }

    struct AgentRun {
        outcome: anyhow::Result<Outcome>,
        output: String,
        _repository: assert_fs::TempDir,
    }

    fn run_agent(writes: AgentWrites, exit: anyhow::Result<AgentExit>) -> AgentRun {
        let directory = repository();
        let root = repository_root(&directory);
        let catalog = root.join("docs/flows");
        let mut input = Cursor::new("1\n");
        let mut output = Vec::new();
        let mut writes = Some(writes);
        let mut exit = Some(exit);
        let mut launch = |command: Command| {
            assert_agent_command(&command, &root, "docs/flows");
            assert!(catalog.is_dir());
            for (install_dir, _) in casemap::skill::INSTALL_DIRS {
                assert!(root.join(install_dir).join("casemap/SKILL.md").is_file());
            }
            write_agent_flow(&catalog, writes.take().unwrap())?;
            exit.take().unwrap()
        };
        let outcome = run(Session {
            cwd: &root,
            dir: Path::new("docs/flows"),
            state: CatalogState::Missing,
            agents: vec![(Agent::Claude, "/x/claude".into())],
            input: &mut input,
            output: &mut output,
            launch: &mut launch,
        });
        AgentRun {
            outcome,
            output: String::from_utf8(output).unwrap(),
            _repository: directory,
        }
    }

    isolated_git_test!(onboarding_runs_init_agent_check_then_browses, {
        let AgentRun {
            outcome, output, ..
        } = run_agent(AgentWrites::ValidFlow, Ok(AgentExit::Success));
        assert!(
            matches!(outcome.unwrap(), Outcome::Browse(path) if path.ends_with("docs/flows") && path.is_relative())
        );
        assert!(output.contains("checked 1 flows:"));
    });

    isolated_git_test!(onboarding_without_new_flows_explains_retry, {
        let AgentRun {
            outcome, output, ..
        } = run_agent(AgentWrites::Nothing, Ok(AgentExit::Success));
        assert!(matches!(outcome.unwrap(), Outcome::Exit(ExitCode::FAILURE)));
        assert!(output.contains("No flows were written"));
    });

    isolated_git_test!(launch_spawn_error_is_an_error, {
        let AgentRun { outcome, .. } =
            run_agent(AgentWrites::Nothing, Err(anyhow::anyhow!("spawn failed")));
        let error = outcome.unwrap_err();
        assert!(error.to_string().contains("starting /x/claude"));
        assert!(format!("{error:#}").contains("spawn failed"));
    });

    isolated_git_test!(nonzero_agent_exit_still_checks, {
        let AgentRun {
            outcome, output, ..
        } = run_agent(
            AgentWrites::ValidFlow,
            Ok(AgentExit::Failed("code 1".into())),
        );
        assert!(matches!(outcome.unwrap(), Outcome::Browse(_)));
        assert!(output.contains("Claude Code exited with code 1"));
        assert!(output.contains("checked 1 flows:"));
    });

    isolated_git_test!(check_failure_after_agent_is_an_error, {
        let AgentRun {
            outcome, output, ..
        } = run_agent(AgentWrites::RemoveCatalog, Ok(AgentExit::Success));
        assert!(
            outcome
                .unwrap_err()
                .to_string()
                .contains("checking the catalog")
        );
        assert!(!output.contains("casemap:"));
    });

    isolated_git_test!(flows_with_problems_still_browse, {
        let AgentRun {
            outcome, output, ..
        } = run_agent(AgentWrites::InvalidFlow, Ok(AgentExit::Success));
        assert!(matches!(outcome.unwrap(), Outcome::Browse(_)));
        assert!(
            output.contains("no one-sentence description after the title"),
            "{output}"
        );
        assert!(output.contains("docs/flows/checkout.md"));
    });

    isolated_git_test!(onboarding_from_a_subdirectory_launches_at_the_root, {
        let directory = repository();
        let root = repository_root(&directory);
        let cwd = root.join("web");
        fs::create_dir(&cwd).unwrap();
        let catalog = root.join("web/docs/flows");
        let mut input = Cursor::new("1\n");
        let mut output = Vec::new();
        let mut launch = |command: Command| {
            assert_agent_command(&command, &root, "web/docs/flows");
            write_agent_flow(&catalog, AgentWrites::ValidFlow)?;
            Ok(AgentExit::Success)
        };
        let outcome = run(Session {
            cwd: &cwd,
            dir: Path::new("docs/flows"),
            state: CatalogState::Missing,
            agents: vec![(Agent::Claude, "/x/claude".into())],
            input: &mut input,
            output: &mut output,
            launch: &mut launch,
        })
        .unwrap();
        assert!(matches!(outcome, Outcome::Browse(path) if path == Path::new("docs/flows")));
        assert!(catalog.join("checkout.md").is_file());
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("per use case in web/docs/flows/"));
        assert!(output.contains("Opening Claude Code with:"));
        assert!(output.contains("checked 1 flows:"));
    });

    #[test]
    fn find_agent_skips_relative_and_empty_path_entries() {
        let absolute = std::env::temp_dir().join("b");
        let path = std::env::join_paths([
            PathBuf::new(),
            PathBuf::from("."),
            PathBuf::from("bin"),
            absolute.clone(),
        ])
        .unwrap();
        assert_eq!(
            find_agent(Agent::Claude, Some(&path), false, &|_| true),
            Some(absolute.join("claude"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn is_executable_requires_the_execute_bit() {
        use std::os::unix::fs::PermissionsExt;
        let directory = assert_fs::TempDir::new().unwrap();
        let program = directory.path().join("claude");
        fs::write(&program, "agent").unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&program));
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&program));
        assert!(!is_executable(directory.path()));
    }

    #[test]
    fn preflight_cases() {
        let agents = vec![(Agent::Codex, PathBuf::from("/x/codex"))];
        assert!(
            preflight(Some(Agent::Codex), false, &agents)
                .unwrap_err()
                .to_string()
                .contains("needs a terminal")
        );
        assert_eq!(
            preflight(Some(Agent::Codex), true, &[])
                .unwrap_err()
                .to_string(),
            "`codex` was not found on PATH"
        );
        assert_eq!(
            preflight(Some(Agent::Codex), true, &agents).unwrap(),
            Some((Agent::Codex, PathBuf::from("/x/codex")))
        );
        assert_eq!(preflight(None, false, &[]).unwrap(), None);
    }

    isolated_git_test!(empty_catalog_menu_heading, {
        let (outcome, output, _) = run_menu(
            "",
            vec![(Agent::Claude, "/x/claude".into())],
            CatalogState::Empty,
        );
        assert!(matches!(outcome, Outcome::Exit(ExitCode::SUCCESS)));
        assert!(output.contains("The use-case map in docs/flows/ is empty."));
    });

    #[test]
    fn run_with_flows_browses_without_output() {
        let (outcome, output, _) = run_menu("", vec![], CatalogState::HasFlows);
        assert!(matches!(outcome, Outcome::Browse(path) if path == Path::new("docs/flows")));
        assert!(output.is_empty());
    }

    /// Writes `text` to `path` and returns both, so a later init that rewrites the file is caught.
    fn overwrite_with_custom_contents(path: PathBuf, text: &str) -> (PathBuf, Vec<u8>) {
        fs::write(&path, text).unwrap();
        (path, text.as_bytes().to_vec())
    }

    fn run_offer(input: &str, expect_launch: bool) -> (Outcome, String) {
        let directory = repository();
        let root = repository_root(&directory);
        let initialized = crate::init_core(&root, Path::new("docs/flows"), false).unwrap();
        let mut preserved = vec![overwrite_with_custom_contents(
            initialized.catalog.join("README.md"),
            "custom catalog notes",
        )];
        for (install_dir, _) in casemap::skill::INSTALL_DIRS {
            preserved.push(overwrite_with_custom_contents(
                root.join(install_dir).join("casemap/SKILL.md"),
                "custom skill instructions",
            ));
        }
        let mut input = Cursor::new(input);
        let mut output = Vec::new();
        let mut launched = false;
        let mut launch = |command: Command| {
            assert_agent_command(&command, &root, "docs/flows");
            for (path, contents) in &preserved {
                assert_eq!(fs::read(path).unwrap(), *contents);
            }
            launched = true;
            write_agent_flow(&initialized.catalog, AgentWrites::ValidFlow)?;
            Ok(AgentExit::Success)
        };
        let mut session = Session {
            cwd: &root,
            dir: Path::new("docs/flows"),
            state: CatalogState::Empty,
            agents: vec![(Agent::Claude, "/x/claude".into())],
            input: &mut input,
            output: &mut output,
            launch: &mut launch,
        };
        let outcome = offer(
            &mut session,
            &initialized.root,
            &initialized.catalog,
            &initialized.display,
        )
        .unwrap();
        assert_eq!(launched, expect_launch);
        for (path, contents) in preserved {
            assert_eq!(fs::read(path).unwrap(), contents);
        }
        (outcome, String::from_utf8(output).unwrap())
    }

    isolated_git_test!(offer_enter_skips_without_launching, {
        let (outcome, _) = run_offer("\n", false);
        assert!(matches!(outcome, Outcome::Exit(ExitCode::SUCCESS)));
    });

    isolated_git_test!(offer_pick_launches_without_reinitialising, {
        let (outcome, output) = run_offer("1\n", true);
        assert!(matches!(outcome, Outcome::Browse(_)));
        assert!(output.contains("Use the casemap skill"));
    });

    isolated_git_test!(offer_prompt_says_skip, {
        let (_, output) = run_offer("\n", false);
        assert!(output.contains("Choose 1-1 (Enter to skip): "));
        assert!(!output.contains("Just show me the steps"));
    });
}
