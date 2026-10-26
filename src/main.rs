//! casemap CLI: adopt, validate, render and compare a catalog of use-case flows.

use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, bail};
use casemap::flows::{Flow, FlowChange, Stale};
use clap::{CommandFactory, Parser, Subcommand};
use notify::{RecursiveMode, Watcher};
use unicode_width::UnicodeWidthStr;

#[cfg(feature = "tui")]
mod browse;
mod hook;
mod onboard;
use casemap::markdown::is_markdown_path;
use casemap::{Direction, Options, ParseError, anchors, check, coverage, git, skill};

const DEFAULT_FLOWS_DIR: &str = skill::DEFAULT_CATALOG;
const DEFAULT_REF: &str = "HEAD";
const WATCH_HEARTBEAT: Duration = Duration::from_secs(1);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);
const WATCH_MAX_DRAIN: Duration = Duration::from_millis(500);

#[derive(Debug, Parser)]
#[command(
    name = "casemap",
    version,
    about = "The living map of your app's use cases: one Mermaid flow per use case in docs/flows/, written by your coding agent, checked and rendered in the terminal.",
    after_help = "Quick start:
  casemap init          create docs/flows/ and install the agent skill
  (ask your agent)      \"document the use-case flows of this repo\"
  casemap browse        see them
  casemap check         validate them
  casemap diff main     what a branch did to them",
    args_override_self = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Mermaid source or a .md file; stdin when omitted or '-'.
    file: Option<PathBuf>,
    /// Re-render whenever FILE changes (Ctrl-C to quit).
    #[arg(short, long, global = true)]
    watch: bool,
    /// Flow catalog directory (default docs/flows).
    #[arg(long, global = true, value_name = "DIR")]
    flows: Option<PathBuf>,
    /// Override the flowchart direction.
    #[arg(long, global = true, value_parser = parse_direction, value_name = "TD|BT|LR|RL")]
    dir: Option<Direction>,
    /// Use plain ASCII instead of box-drawing characters.
    #[arg(long, global = true)]
    ascii: bool,
    /// Force ANSI colour.
    #[arg(long, global = true, overrides_with = "no_color")]
    color: bool,
    /// Disable ANSI colour (also respects NO_COLOR).
    #[arg(long, global = true, overrides_with = "color")]
    no_color: bool,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum DiffFormat {
    Text,
    Markdown,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create the flow catalog and install the casemap agent skill.
    Init {
        #[arg(long)]
        ci: bool,
        #[arg(long)]
        hooks: bool,
        #[arg(long)]
        force: bool,
        #[arg(long, value_enum)]
        agent: Option<onboard::Agent>,
    },
    /// Agent integration hooks (installed by `casemap init --hooks`).
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// List the flow catalog, or render one flow.
    Flows {
        name: Option<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Explore the flow catalog interactively.
    Browse {
        #[arg(long)]
        base: Option<String>,
    },
    /// Validate the flow catalog for agents and CI.
    Check {
        #[arg(long)]
        strict: bool,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Show catalog changes since REF (default HEAD), including uncommitted edits.
    Diff {
        rev: Option<String>,
        #[arg(long, value_enum, default_value_t = DiffFormat::Text)]
        format: DiffFormat,
        /// Also exit 1 when flows are possibly stale (for local agent runs).
        #[arg(long)]
        strict: bool,
    },
    /// Which flows and steps are anchored to the given files (only anchored flows are known).
    Which {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Which code no flow covers: the whole repository, or the files changed since REF.
    Coverage {
        rev: Option<String>,
        #[arg(long = "include", value_name = "PATTERN")]
        include: Vec<String>,
        #[arg(long = "exclude", value_name = "PATTERN")]
        exclude: Vec<String>,
        /// With REF, exit 1 when changed code is not covered.
        #[arg(long)]
        strict: bool,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Print the casemap agent skill without its front matter.
    Guide,
    /// Generate shell completions.
    Completions { shell: clap_complete::Shell },
}

#[derive(Debug, Subcommand)]
enum HookEvent {
    /// Run when a coding agent is about to finish: block once if the use-case map needs attention.
    Stop,
}

fn parse_direction(value: &str) -> Result<Direction, String> {
    Direction::parse(value).ok_or_else(|| "expects one of TD, BT, LR, RL".into())
}

fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("hook") {
        std::panic::set_hook(Box::new(|_| {}));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Ok(cli) = Cli::try_parse() {
                let _ = run(cli);
            }
        }));
        return ExitCode::SUCCESS;
    }
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(if error.use_stderr() { 1 } else { 0 });
        }
    };
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("casemap: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    if cli.command.is_some() && cli.watch {
        bail!("--watch only applies to rendering a file");
    }
    if cli.command.is_some() && cli.file.is_some() {
        bail!("FILE cannot be combined with a subcommand");
    }
    let color = resolve_color(&cli);
    let opts = Options {
        color,
        ascii: cli.ascii,
        direction: cli.dir,
    };
    #[cfg(feature = "tui")]
    let browse_opts = Options {
        color: !cli.no_color && std::env::var_os("NO_COLOR").is_none(),
        ..opts.clone()
    };
    let directory = cli
        .flows
        .as_deref()
        .unwrap_or_else(|| Path::new(DEFAULT_FLOWS_DIR));
    #[cfg(feature = "tui")]
    let onboarding_opts = &browse_opts;
    #[cfg(not(feature = "tui"))]
    let onboarding_opts = &opts;
    match cli.command {
        Some(Commands::Diff {
            rev,
            strict,
            format,
        }) => diff_catalog(
            directory,
            rev.as_deref().unwrap_or(DEFAULT_REF),
            strict,
            format,
            &opts,
        ),
        #[cfg(feature = "tui")]
        Some(Commands::Browse { base }) => browse::run(directory, base.as_deref(), &browse_opts),
        #[cfg(not(feature = "tui"))]
        Some(Commands::Browse { .. }) => bail!("casemap was built without the tui feature"),
        Some(Commands::Which { paths, format }) => which_flows(directory, &paths, format),
        Some(Commands::Coverage {
            rev,
            include,
            exclude,
            strict,
            format,
        }) => measure_coverage(
            directory,
            rev.as_deref(),
            &include,
            &exclude,
            strict,
            format,
        ),
        Some(Commands::Init {
            force,
            agent,
            ci,
            hooks,
        }) => {
            let outcome = init_catalog(directory, force, agent, ci, hooks)?;
            finish_onboarding(outcome, onboarding_opts)
        }
        Some(Commands::Flows { name, format }) => {
            show_flows(directory, name.as_deref(), format, &opts)
        }
        Some(Commands::Check { strict, format }) => check_catalog(directory, strict, format, color),
        Some(Commands::Hook {
            event: HookEvent::Stop,
        }) => {
            hook::stop(cli.flows.as_deref());
            Ok(ExitCode::SUCCESS)
        }
        Some(Commands::Guide) => {
            print_guide(directory);
            Ok(ExitCode::SUCCESS)
        }
        Some(Commands::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "casemap", &mut io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        None => {
            let mode = onboard::bare_mode(&onboard::BareModeInputs {
                interactive: interactive_session(),
                catalog: onboard::catalog_state(directory),
                watch: cli.watch,
                has_file: cli.file.is_some(),
                browser_available: cfg!(feature = "tui"),
            });
            match mode {
                onboard::BareMode::Browse => finish_onboarding(
                    onboard::Outcome::Browse(directory.to_path_buf()),
                    onboarding_opts,
                ),
                onboard::BareMode::Onboard => {
                    let cwd = std::env::current_dir().context("reading the current directory")?;
                    let outcome = with_terminal_session(
                        &cwd,
                        directory,
                        onboard::catalog_state(directory),
                        onboard::detect_agents(),
                        onboard::run,
                    )?;
                    finish_onboarding(outcome, onboarding_opts)
                }
                onboard::BareMode::ReadStdin => render_file(cli.file.as_deref(), cli.watch, &opts),
            }
        }
    }
}

#[cfg(feature = "tui")]
fn finish_onboarding(outcome: onboard::Outcome, options: &Options) -> anyhow::Result<ExitCode> {
    match outcome {
        onboard::Outcome::Exit(code) => Ok(code),
        onboard::Outcome::Browse(catalog) => browse::run(&catalog, None, options),
    }
}

#[cfg(not(feature = "tui"))]
fn finish_onboarding(outcome: onboard::Outcome, options: &Options) -> anyhow::Result<ExitCode> {
    match outcome {
        onboard::Outcome::Exit(code) => Ok(code),
        onboard::Outcome::Browse(catalog) => show_flows(&catalog, None, Format::Text, options),
    }
}

struct TerminalMenuInput;

impl onboard::MenuInput for TerminalMenuInput {
    fn read_menu_line(&mut self, line: &mut String) -> io::Result<usize> {
        // Canonical tty mode returns one line per read, leaving typed-ahead lines for the agent.
        io::stdin().lock().read_line(line)
    }
}

fn with_terminal_session(
    cwd: &Path,
    dir: &Path,
    state: onboard::CatalogState,
    agents: Vec<(onboard::Agent, PathBuf)>,
    action: impl FnOnce(onboard::Session<'_>) -> anyhow::Result<onboard::Outcome>,
) -> anyhow::Result<onboard::Outcome> {
    let mut input = TerminalMenuInput;
    let mut output = io::stdout();
    action(onboard::Session {
        cwd,
        dir,
        state,
        agents,
        input: &mut input,
        output: &mut output,
        launch: &mut onboard::launch,
    })
}

fn interactive_session() -> bool {
    onboard::is_interactive_session(
        io::stdin().is_terminal(),
        io::stdout().is_terminal(),
        std::env::var_os("CI").is_some(),
        std::env::var_os("TERM").is_some_and(|term| term == "dumb"),
    )
}

fn resolve_color(cli: &Cli) -> bool {
    if cli.no_color {
        false
    } else if cli.color {
        true
    } else {
        io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
    }
}

fn is_stdin(path: &Path) -> bool {
    path == Path::new("-")
}

fn read_source(file: Option<&Path>) -> anyhow::Result<String> {
    match file {
        None => read_stdin(),
        Some(path) if is_stdin(path) => read_stdin(),
        Some(path) => std::fs::read_to_string(path).with_context(|| path.display().to_string()),
    }
}

fn render_file(file: Option<&Path>, watch: bool, opts: &Options) -> anyhow::Result<ExitCode> {
    if watch {
        return match file {
            Some(path) if !is_stdin(path) => watch_file(path, opts),
            _ => bail!("--watch needs a file path"),
        };
    }
    let source = read_source(file)?;
    print!("{}", render_source(file, &source, opts)?);
    Ok(ExitCode::SUCCESS)
}

fn print_guide(directory: &Path) {
    let catalog = catalog_path(directory);
    print!("{}", skill::body(&catalog));
    println!("\nExample file: `{catalog}/check-out-a-cart.md`.");
}

fn read_stdin() -> anyhow::Result<String> {
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .context("reading stdin")?;
    Ok(source)
}

#[derive(Debug, thiserror::Error)]
enum RenderError {
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("{0}: no mermaid blocks")]
    NoBlocks(String),
}

fn render_source(path: Option<&Path>, src: &str, opts: &Options) -> Result<String, RenderError> {
    let Some(path) = path.filter(|path| is_markdown_path(path)) else {
        return Ok(casemap::render_str(src, opts)?);
    };
    if casemap::markdown::mermaid_blocks(src).is_empty() {
        return Err(RenderError::NoBlocks(path.display().to_string()));
    }
    Ok(casemap::render_markdown(src, opts)?)
}

fn require_catalog(dir: &Path) -> anyhow::Result<()> {
    if !dir.is_dir() {
        bail!(
            "no flow catalog at {} (create it or pass --flows <dir>)",
            dir.display()
        );
    }
    Ok(())
}

fn load_catalog(dir: &Path) -> anyhow::Result<Vec<Flow>> {
    require_catalog(dir)?;
    Ok(casemap::flows::load_dir(dir)?)
}

struct BaseChanges {
    base: String,
    #[cfg_attr(not(feature = "tui"), allow(dead_code))]
    old: Vec<Flow>,
    changes: Vec<FlowChange>,
    stale: Vec<Stale>,
    changed: Vec<String>,
    root: PathBuf,
    prefix: String,
}

fn has_code_anchors(current: &[Flow]) -> bool {
    current
        .iter()
        .any(|flow| !anchors::flow_anchors(flow).0.is_empty())
}

fn base_changes(dir: &Path, rev: &str, current: &[Flow]) -> anyhow::Result<BaseChanges> {
    let base = casemap::git::merge_base(dir, rev)?;
    let old = casemap::git::files_at(dir, &base)?
        .into_iter()
        .map(|(path, text)| casemap::flows::parse_flow(&path, &text))
        .collect::<Vec<_>>();
    let changes = casemap::flows::compare(&old, current);
    let (root, _, prefix) = code_root(dir)?;
    let (changed, stale) = if has_code_anchors(current) {
        let changed = git::changed_files(&root, &base)?;
        let stale = casemap::flows::stale_flows(current, &prefix, &changed);
        (changed, stale)
    } else {
        (Vec::new(), Vec::new())
    };
    Ok(BaseChanges {
        base,
        old,
        changes,
        stale,
        changed,
        root,
        prefix,
    })
}

fn diff_catalog(
    dir: &Path,
    rev: &str,
    strict: bool,
    format: DiffFormat,
    opts: &Options,
) -> anyhow::Result<ExitCode> {
    if matches!(format, DiffFormat::Markdown) {
        return diff_catalog_markdown(dir, rev, strict);
    }
    let new = load_catalog(dir)?;
    let BaseChanges {
        base,
        changes,
        stale,
        ..
    } = base_changes(dir, rev, &new)?;
    print!(
        "{}",
        casemap::flows::render_report(&changes, &stale, rev, &base, opts)
    );
    if changes
        .iter()
        .any(|change| matches!(change, casemap::flows::FlowChange::Broken { .. }))
        || (strict && !stale.is_empty())
    {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn diff_catalog_markdown(dir: &Path, rev: &str, strict: bool) -> anyhow::Result<ExitCode> {
    let current = load_catalog(dir)?;
    let mut compared = base_changes(dir, rev, &current)?;
    for change in &mut compared.changes {
        restore_removed_flow(change, &compared.old);
    }
    let report = check_report(dir)?;
    let changed = changed_code(
        &compared.changed,
        &compared.prefix,
        &coverage::Selection::default(),
        &|path| compared.root.join(path).is_file(),
    );
    let uncovered = if casemap::flows::has_known_anchors(&current) {
        coverage::uncovered_changes(&changed, &current)
    } else {
        Vec::new()
    };
    print!(
        "{}",
        casemap::flows::render_markdown(&casemap::flows::MarkdownInput {
            changes: &compared.changes,
            stale: &compared.stale,
            since: rev,
            base: &compared.base,
            report: &report,
            catalog: &catalog_path(dir),
            uncovered: &uncovered,
        })
    );
    Ok(markdown_exit_code(
        &report,
        &compared.changes,
        &compared.stale,
        strict,
    ))
}

fn restore_removed_flow(change: &mut FlowChange, old: &[Flow]) {
    let FlowChange::Removed { flow } = change else {
        return;
    };
    if flow.source.is_some() {
        return;
    }
    let Some(previous) = old.iter().find(|previous| previous.path == flow.path) else {
        return;
    };
    *flow = previous.clone();
}

fn markdown_exit_code(
    report: &check::Report,
    changes: &[FlowChange],
    stale: &[Stale],
    strict: bool,
) -> ExitCode {
    let broken = changes
        .iter()
        .any(|change| matches!(change, FlowChange::Broken { .. }));
    if report.errors() > 0 || broken || (strict && !stale.is_empty()) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn show_flows(
    dir: &Path,
    name: Option<&str>,
    format: Format,
    opts: &Options,
) -> anyhow::Result<ExitCode> {
    if name.is_some() && matches!(format, Format::Json) {
        bail!("--format json lists the catalog; drop the flow name");
    }
    let mut flows = load_catalog(dir)?;
    if matches!(format, Format::Json) {
        let entries: Vec<_> = flows
            .iter()
            .map(|flow| {
                serde_json::json!({
                    "name": flow.name,
                    "path": catalog_file_path(dir, &flow.path),
                    "title": flow.title,
                    "description": flow.description,
                    "has_diagram": flow.source.is_some(),
                })
            })
            .collect();
        println!("{}", serde_json::json!({"version": 1, "flows": entries}));
        return Ok(ExitCode::SUCCESS);
    }
    flows.sort_by(casemap::flows::compare_titles);
    let Some(name) = name else {
        if flows.is_empty() {
            println!("no flows in {}", dir.display());
        } else {
            print!("{}", casemap::flows::render_list(&flows));
        }
        return Ok(ExitCode::SUCCESS);
    };
    let Some(flow) = flows.iter().find(|flow| flow.name == name) else {
        let available: Vec<&str> = flows.iter().map(|flow| flow.name.as_str()).collect();
        bail!("no flow named {name}; available: {}", available.join(", "));
    };
    let diagram = match casemap::flows::diagram(flow) {
        Some(Ok(diagram)) => diagram,
        Some(Err(error)) => bail!("{}: {error}", flow.path),
        None => bail!("{}: no mermaid block", flow.path),
    };
    print!("{}", casemap::render::bold_line(&flow.title, opts.color));
    if let Some(description) = &flow.description {
        println!("{description}");
    }
    println!();
    print!("{}", casemap::render_diagram(&diagram, opts));
    print!("{}", anchors::render_footer(flow));
    Ok(ExitCode::SUCCESS)
}

#[derive(Clone, Copy)]
enum FileSource {
    Git,
    Walk,
}

fn falls_back_to_walk(error: &git::GitError) -> bool {
    matches!(error, git::GitError::Spawn { error } if error.kind() == io::ErrorKind::NotFound)
}

fn walk_root() -> anyhow::Result<(PathBuf, FileSource, String)> {
    Ok((
        std::env::current_dir().context("reading the current directory")?,
        FileSource::Walk,
        String::new(),
    ))
}

fn code_root(catalog_dir: &Path) -> anyhow::Result<(PathBuf, FileSource, String)> {
    match git::locate(catalog_dir) {
        Ok(git::Location::Inside { root, prefix }) => Ok((root, FileSource::Git, prefix)),
        Ok(git::Location::NotARepository) => walk_root(),
        Err(error) if falls_back_to_walk(&error) => walk_root(),
        Err(error) => Err(error.into()),
    }
}

struct LazyCodeIndex {
    catalog_dir: PathBuf,
    location: Option<(PathBuf, FileSource)>,
    files: Option<Vec<String>>,
    error: Option<anyhow::Error>,
}

impl LazyCodeIndex {
    fn deferred(catalog_dir: &Path) -> Self {
        Self {
            catalog_dir: catalog_dir.to_path_buf(),
            location: None,
            files: None,
            error: None,
        }
    }

    fn root(&mut self) -> anyhow::Result<&Path> {
        Ok(&self.ensure_location()?.0)
    }

    fn ensure_location(&mut self) -> anyhow::Result<&(PathBuf, FileSource)> {
        if let Some(ref location) = self.location {
            return Ok(location);
        }
        let (root, source, _) = code_root(&self.catalog_dir)?;
        Ok(self.location.insert((root, source)))
    }

    fn files(&mut self) -> anyhow::Result<&[String]> {
        if self.files.is_none() {
            let (root, source) = self.ensure_location()?;
            self.files = Some(code_files(root, *source)?);
        }
        Ok(self.files.as_deref().unwrap_or_default())
    }

    /// Answers true after an index failure so no false anchor-no-match is reported; take_error returns the failure.
    fn has_match(&mut self, pattern: &str) -> bool {
        if self.error.is_some() {
            return true;
        }
        match self.files() {
            Ok(files) => files
                .iter()
                .any(|path| anchors::pattern_matches(pattern, path)),
            Err(error) => {
                self.error = Some(error);
                true
            }
        }
    }

    fn finish(mut self) -> anyhow::Result<()> {
        match self.take_error() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn take_error(&mut self) -> Option<anyhow::Error> {
        self.error.take()
    }
}

fn code_files(root: &Path, source: FileSource) -> anyhow::Result<Vec<String>> {
    match source {
        FileSource::Git => Ok(git::repo_files(root)?),
        FileSource::Walk => anchors::walk_files(root)
            .with_context(|| format!("reading files in {}", root.display())),
    }
}

fn deepest_existing_ancestor(path: &Path) -> anyhow::Result<(PathBuf, Vec<Component<'_>>)> {
    let mut ancestor = path;
    while std::fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor
            .parent()
            .with_context(|| format!("resolving {}", path.display()))?;
    }
    let tail = path.strip_prefix(ancestor)?.components().collect();
    Ok((ancestor.to_path_buf(), tail))
}

fn replay_tail(mut resolved: PathBuf, tail: Vec<Component<'_>>) -> PathBuf {
    for component in tail {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            _ => resolved.push(component),
        }
    }
    resolved
}

fn root_relative_path(path: &Path, root: &Path) -> anyhow::Result<String> {
    let absolute = std::env::current_dir()
        .context("reading the current directory")?
        .join(path);
    let (ancestor, tail) = deepest_existing_ancestor(&absolute)?;
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let resolved = ancestor
        .canonicalize()
        .with_context(|| format!("resolving {}", ancestor.display()))?;
    let resolved = replay_tail(resolved, tail);
    let Ok(relative) = resolved.strip_prefix(&canonical_root) else {
        bail!("{} is outside the repository", path.display());
    };
    Ok(slash_path(relative))
}

struct WhichPath<'a> {
    path: String,
    flows: Vec<casemap::flows::AnchoredFlow<'a>>,
}

fn which_flows(dir: &Path, paths: &[PathBuf], format: Format) -> anyhow::Result<ExitCode> {
    let flows = load_catalog(dir)?;
    let mut index = LazyCodeIndex::deferred(dir);
    let root = index.root()?.to_path_buf();
    let mut results = Vec::new();
    for path in paths {
        let relative = root_relative_path(path, &root)?;
        let files = if root.join(&relative).is_dir() {
            index
                .files()?
                .iter()
                .filter(|file| file_is_under_directory(file, &relative))
                .cloned()
                .collect()
        } else {
            vec![relative.clone()]
        };
        results.push(WhichPath {
            path: if relative.is_empty() {
                ".".into()
            } else {
                relative
            },
            flows: casemap::flows::anchored_flows(&flows, &files),
        });
    }
    match format {
        Format::Text => print!("{}", render_which_text(&results)),
        Format::Json => print_which_json(dir, &root, &results)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn selection_patterns(values: &[String], option: &str) -> anyhow::Result<Vec<String>> {
    values
        .iter()
        .map(|value| {
            anchors::normalise_pattern(value)
                .map_err(|error| anyhow::anyhow!("invalid {option} pattern {error}"))
        })
        .collect()
}

fn measure_coverage(
    dir: &Path,
    rev: Option<&str>,
    include: &[String],
    exclude: &[String],
    strict: bool,
    format: Format,
) -> anyhow::Result<ExitCode> {
    let flows = load_catalog(dir)?;
    let selection = coverage::Selection {
        include: selection_patterns(include, "--include")?,
        exclude: selection_patterns(exclude, "--exclude")?,
    };
    match rev {
        Some(rev) => coverage_branch(dir, rev, &flows, &selection, strict, format),
        None => coverage_repository(dir, &flows, &selection, format),
    }
}

fn changed_code(
    changed: &[String],
    catalog_prefix: &str,
    selection: &coverage::Selection,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    changed
        .iter()
        .filter(|path| exists(path) && coverage::is_code_path(path, catalog_prefix, selection))
        .cloned()
        .collect()
}

fn coverage_branch(
    dir: &Path,
    rev: &str,
    flows: &[Flow],
    selection: &coverage::Selection,
    strict: bool,
    format: Format,
) -> anyhow::Result<ExitCode> {
    let (root, _, catalog_prefix) = code_root(dir)?;
    let base = git::merge_base(dir, rev)?;
    let changed = changed_code(
        &git::changed_files(&root, &base)?,
        &catalog_prefix,
        selection,
        &|path| root.join(path).is_file(),
    );
    let anchored = casemap::flows::has_known_anchors(flows);
    let uncovered = if anchored {
        coverage::uncovered_changes(&changed, flows)
    } else {
        Vec::new()
    };
    match format {
        Format::Json => println!(
            "{}",
            serde_json::json!({
                "version": 1, "mode": "branch", "since": rev, "base": base,
                "changed": changed.len(), "uncovered": uncovered, "anchored": anchored,
            })
        ),
        Format::Text => {
            if anchored {
                print!(
                    "{}",
                    coverage_branch_text(rev, &base, changed.len(), &uncovered)
                );
            } else {
                print!("{NO_COVERAGE_ANCHORS}");
            }
        }
    }
    Ok(if strict && !uncovered.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn directory_json(directory: &coverage::Directory) -> serde_json::Value {
    serde_json::json!({
        "path": directory.path, "files": directory.files, "covered": directory.covered,
        "uncovered": directory.uncovered,
    })
}

fn coverage_repository(
    dir: &Path,
    flows: &[Flow],
    selection: &coverage::Selection,
    format: Format,
) -> anyhow::Result<ExitCode> {
    let mut index = LazyCodeIndex::deferred(dir);
    let root = index.root()?.to_path_buf();
    let catalog_prefix = root_relative_path(dir, &root)?;
    let report = coverage::repository(index.files()?, flows, &catalog_prefix, selection);
    match format {
        Format::Text => print!("{}", coverage_repository_text(&report)),
        Format::Json => {
            let directories: Vec<_> = report.directories.iter().map(directory_json).collect();
            println!(
                "{}",
                serde_json::json!({
                    "version": 1, "mode": "repository", "files": report.files, "covered": report.covered,
                    "anchored": report.anchored, "directories": directories,
                })
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

const NO_COVERAGE_ANCHORS: &str =
    "no flow has @code anchors yet; anchor steps to code to measure coverage\n";

fn coverage_file_word(count: usize) -> &'static str {
    if count == 1 { "file" } else { "files" }
}

fn coverage_branch_text(rev: &str, base: &str, changed: usize, uncovered: &[String]) -> String {
    let short: String = base.chars().take(casemap::flows::SHORT_SHA_LEN).collect();
    if changed == 0 {
        return format!("no code changed since {rev} ({short})\n");
    }
    let files = coverage_file_word(changed);
    if uncovered.is_empty() {
        return format!(
            "every changed code file since {rev} ({short}) is covered by a flow ({changed} {files})\n"
        );
    }
    let mut output = format!(
        "changed code no flow covers since {rev} ({short}): {} of {changed} {files}\n",
        uncovered.len()
    );
    for path in uncovered {
        output.push_str(&format!("  {path}\n"));
    }
    output
}

const MAX_UNCOVERED_NAMES: usize = 5;

fn coverage_percentage(covered: usize, files: usize) -> usize {
    covered * 100 / files
}

fn coverage_directory_label(path: &str) -> String {
    if path.is_empty() {
        ".".into()
    } else {
        format!("{path}/")
    }
}

fn coverage_repository_text(report: &coverage::Report) -> String {
    if !report.anchored {
        return NO_COVERAGE_ANCHORS.into();
    }
    if report.files == 0 {
        return "no code files found (adjust --include)\n".into();
    }
    let percentage = coverage_percentage(report.covered, report.files);
    let mut output = format!(
        "code anchored to flows: {percentage}% ({} of {} {})\n\n",
        report.covered,
        report.files,
        coverage_file_word(report.files)
    );
    let width = report
        .directories
        .iter()
        .map(|directory| UnicodeWidthStr::width(coverage_directory_label(&directory.path).as_str()))
        .max()
        .unwrap_or(0);
    for directory in &report.directories {
        let label = coverage_directory_label(&directory.path);
        let percentage = coverage_percentage(directory.covered, directory.files);
        let padding = " ".repeat(width - UnicodeWidthStr::width(label.as_str()));
        output.push_str(&format!(
            "  {label}{padding}  {percentage:>3}%   {}/{}",
            directory.covered, directory.files
        ));
        if !directory.uncovered.is_empty() {
            let names: Vec<_> = directory
                .uncovered
                .iter()
                .take(MAX_UNCOVERED_NAMES)
                .map(String::as_str)
                .collect();
            output.push_str(&format!("   not covered: {}", names.join(", ")));
            let remaining = directory
                .uncovered
                .len()
                .saturating_sub(MAX_UNCOVERED_NAMES);
            if remaining > 0 {
                output.push_str(&format!(", … (+{remaining} more)"));
            }
        }
        output.push('\n');
    }
    output
}

fn file_is_under_directory(file: &str, directory: &str) -> bool {
    directory.is_empty()
        || file
            .strip_prefix(directory)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn which_annotation(flow: &casemap::flows::AnchoredFlow<'_>) -> String {
    let mut parts = Vec::new();
    if flow.whole_flow {
        parts.push("whole flow".to_string());
    }
    if !flow.steps.is_empty() {
        parts.push(format!("steps: {}", flow.steps.join(", ")));
    }
    parts.join("; ")
}

fn render_which_text(results: &[WhichPath<'_>]) -> String {
    let mut blocks = Vec::new();
    for result in results {
        if result.flows.is_empty() {
            blocks.push(format!("{}: no anchored flow\n", result.path));
            continue;
        }
        let mut block = format!("{}\n", result.path);
        for flow in &result.flows {
            block.push_str(&format!(
                "  {}  {}  ({})\n",
                flow.flow.path,
                flow.flow.title,
                which_annotation(flow)
            ));
        }
        blocks.push(block);
    }
    blocks.join("\n")
}

fn print_which_json(dir: &Path, root: &Path, results: &[WhichPath<'_>]) -> anyhow::Result<()> {
    let catalog = root_relative_path(dir, root)?;
    let mut paths = Vec::new();
    for result in results {
        let flows: Vec<_> = result
            .flows
            .iter()
            .map(|flow| {
                serde_json::json!({
                    "name": flow.flow.name,
                    "path": slash_path(&Path::new(&catalog).join(&flow.flow.path)),
                    "title": flow.flow.title,
                    "steps": flow.steps,
                    "whole_flow": flow.whole_flow,
                })
            })
            .collect();
        paths.push(serde_json::json!({"path": result.path, "flows": flows}));
    }
    println!("{}", serde_json::json!({"version": 1, "paths": paths}));
    Ok(())
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn catalog_path(dir: &Path) -> String {
    slash_path(dir).trim_end_matches('/').to_string()
}

fn catalog_file_path(dir: &Path, file: &str) -> String {
    format!("{}/{}", catalog_path(dir), file)
}

fn severity_text(severity: check::Severity) -> &'static str {
    match severity {
        check::Severity::Error => "error",
        check::Severity::Warning => "warning",
    }
}

fn coloured_severity(severity: check::Severity, color: bool) -> String {
    let text = severity_text(severity);
    if !color {
        return text.to_string();
    }
    let ink = match severity {
        check::Severity::Error => "\x1b[31m",
        check::Severity::Warning => "\x1b[33m",
    };
    format!("{ink}{text}\x1b[0m")
}

fn problem_location(dir: &Path, problem: &check::Problem) -> String {
    let path = match &problem.file {
        Some(file) => catalog_file_path(dir, file),
        None => catalog_path(dir),
    };
    match problem.line {
        Some(line) => format!("{path}:{line}"),
        None => path,
    }
}

fn problem_json(dir: &Path, problem: &check::Problem) -> serde_json::Value {
    serde_json::json!({
        "path": problem.file.as_ref().map(|file| catalog_file_path(dir, file)),
        "line": problem.line,
        "severity": severity_text(problem.code.severity()),
        "code": problem.code.as_str(),
        "message": problem.message,
    })
}

fn check_summary(report: &check::Report) -> String {
    let mut counts = Vec::new();
    for (count, noun) in [(report.errors(), "error"), (report.warnings(), "warning")] {
        if count > 0 {
            let suffix = if count == 1 { "" } else { "s" };
            counts.push(format!("{count} {noun}{suffix}"));
        }
    }
    let result = if counts.is_empty() {
        "ok".to_string()
    } else {
        counts.join(", ")
    };
    format!("checked {} flows: {result}", report.flows)
}

fn check_report(dir: &Path) -> anyhow::Result<check::Report> {
    require_catalog(dir)?;
    let mut index = LazyCodeIndex::deferred(dir);
    let report = check::check_catalog_with(dir, &mut |pattern| index.has_match(pattern))?;
    index.finish()?;
    Ok(report)
}

fn check_catalog(
    dir: &Path,
    strict: bool,
    format: Format,
    color: bool,
) -> anyhow::Result<ExitCode> {
    let report = check_report(dir)?;
    match format {
        Format::Text => print_check_text(dir, &report, color),
        Format::Json => print_check_json(dir, &report),
    }
    Ok(check_exit_code(&report, strict))
}

fn problem_line(problem: &check::Problem, prefix: &str, severity: &str) -> String {
    format!(
        "{prefix}{severity}: {} [{}]",
        problem.message,
        problem.code.as_str()
    )
}

fn print_check_text(dir: &Path, report: &check::Report, color: bool) {
    write_check_text(&mut io::stdout().lock(), dir, report, color).expect("writing check output");
}

fn write_check_text(
    output: &mut dyn Write,
    dir: &Path,
    report: &check::Report,
    color: bool,
) -> io::Result<()> {
    for problem in &report.problems {
        writeln!(
            output,
            "{}",
            problem_line(
                problem,
                &format!("{}: ", problem_location(dir, problem)),
                &coloured_severity(problem.code.severity(), color)
            )
        )?;
    }
    writeln!(output, "{}", check_summary(report))?;
    Ok(())
}

fn print_check_json(dir: &Path, report: &check::Report) {
    let problems: Vec<_> = report
        .problems
        .iter()
        .map(|problem| problem_json(dir, problem))
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "version": 1, "catalog": catalog_path(dir), "flows": report.flows,
            "errors": report.errors(), "warnings": report.warnings(), "problems": problems,
        })
    );
}

fn check_exit_code(report: &check::Report, strict: bool) -> ExitCode {
    if report.errors() > 0 || (strict && report.warnings() > 0) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn catalog_write_error(path: &Path, error: io::Error) -> casemap::flows::CatalogError {
    casemap::flows::CatalogError {
        path: path.to_path_buf(),
        error,
    }
}

fn create_catalog(dir: &Path, display: &str) -> Result<(), casemap::flows::CatalogError> {
    let existed = dir.is_dir();
    std::fs::create_dir_all(dir).map_err(|error| catalog_write_error(dir, error))?;
    let mut entries = std::fs::read_dir(dir).map_err(|error| catalog_write_error(dir, error))?;
    let is_empty = entries
        .next()
        .transpose()
        .map_err(|error| catalog_write_error(dir, error))?
        .is_none();
    if !existed || is_empty {
        let keep = dir.join(".gitkeep");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&keep)
            .map_err(|error| catalog_write_error(&keep, error))?;
    }
    let status = if existed { "exists" } else { "created" };
    println!("{status} {display}/");
    Ok(())
}

fn create_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn existing_metadata(path: &Path) -> io::Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".casemap-tmp");
    PathBuf::from(name)
}

fn remove_stale_temporary(path: &Path) -> io::Result<()> {
    if existing_metadata(path)?.is_some() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn replace_through_temporary(
    path: &Path,
    temporary: &Path,
    desired: &str,
    metadata: Option<std::fs::Metadata>,
) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)?;
    file.write_all(desired.as_bytes())?;
    file.sync_all()?;
    if let Some(metadata) = metadata {
        std::fs::set_permissions(temporary, metadata.permissions())?;
    }
    drop(file);
    if let Err(error) = std::fs::rename(temporary, path) {
        eprintln!(
            "warning: {}: cannot rename temporary skill: {error}; writing directly",
            path.display()
        );
        std::fs::write(path, desired)?;
    }
    Ok(())
}

fn write_skill(path: &Path, desired: &str) -> io::Result<()> {
    create_parent(path)?;
    let metadata = existing_metadata(path)?;
    if metadata
        .as_ref()
        .is_some_and(|metadata| metadata.is_symlink())
    {
        return std::fs::write(path, desired);
    }
    let temporary = temporary_path(path);
    remove_stale_temporary(&temporary)?;
    let result = replace_through_temporary(path, &temporary, desired, metadata);
    let _ = std::fs::remove_file(&temporary);
    result
}

#[derive(Debug, PartialEq, Eq)]
enum SkillAction {
    Install,
    UpToDate,
    Keep,
    Update,
}

fn decide_skill_action(current: Option<&str>, desired: &str, force: bool) -> SkillAction {
    let Some(current) = current else {
        return SkillAction::Install;
    };
    if skill::normalise(current) == skill::normalise(desired) {
        SkillAction::UpToDate
    } else if force {
        SkillAction::Update
    } else {
        SkillAction::Keep
    }
}

fn check_skill_symlinks(root: &Path, relative: &Path) -> io::Result<()> {
    let mut component_path = PathBuf::new();
    for component in relative.components() {
        component_path.push(component);
        let path = root.join(&component_path);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        };
        if metadata.is_symlink() {
            match std::fs::metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(io::Error::other(format!(
                        "{} is a symlink to a missing path",
                        slash_path(&component_path)
                    )));
                }
                Err(error) => return Err(error),
                Ok(_) => {}
            }
        }
    }
    Ok(())
}

fn install_skill(
    root: &Path,
    relative: &Path,
    desired: &str,
    force: bool,
    agent_label: &str,
    what: &str,
) -> io::Result<()> {
    check_skill_symlinks(root, relative)?;
    let path = root.join(relative);
    let display = slash_path(relative);
    let current = match std::fs::read(&path) {
        Ok(current) => Some(String::from_utf8_lossy(&current).into_owned()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    match decide_skill_action(current.as_deref(), desired, force) {
        SkillAction::Install => {
            write_skill(&path, desired)?;
            println!("installed {display} ({agent_label})");
        }
        SkillAction::UpToDate => println!("up to date {display}"),
        SkillAction::Keep => {
            println!("kept {display} (differs from this casemap version; use --force to update)")
        }
        SkillAction::Update => {
            write_skill(&path, desired)?;
            println!("updated {display}");
        }
    }
    if git::is_ignored(root, relative) {
        println!("warning: {display} is gitignored; teammates will not get {what}");
    }
    Ok(())
}

fn catalog_display(cwd: &Path, dir: &Path) -> anyhow::Result<(PathBuf, String)> {
    let (root, prefix) = match git::locate(cwd)? {
        git::Location::Inside { root, prefix } => (root, prefix),
        git::Location::NotARepository => (cwd.to_path_buf(), String::new()),
    };
    let relative = normalise_catalog_path(&prefix, dir)?;
    Ok((root, catalog_path(&relative)))
}

fn normalise_catalog_path(prefix: &str, dir: &Path) -> anyhow::Result<PathBuf> {
    use std::path::Component;
    if dir.is_absolute() || dir.has_root() {
        bail!("--flows must stay inside the repository");
    }
    let mut normalised = PathBuf::new();
    for component in Path::new(prefix).join(dir).components() {
        match component {
            Component::Normal(name) => normalised.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalised.pop() {
                    bail!("--flows must stay inside the repository");
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!("--flows must stay inside the repository")
            }
        }
    }
    if normalised.as_os_str().is_empty() {
        bail!("--flows must name a directory inside the repository");
    }
    Ok(normalised)
}

fn ensure_inside_root(root: &Path, relative: &Path) -> anyhow::Result<()> {
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("resolving {}", root.display()))?;
    let mut ancestor = root.join(relative);
    while std::fs::symlink_metadata(&ancestor).is_err() {
        if !ancestor.pop() {
            bail!("--flows must stay inside the repository");
        }
    }
    let canonical_ancestor = match ancestor
        .canonicalize()
        .with_context(|| format!("resolving {}", ancestor.display()))
    {
        Ok(canonical) => canonical,
        Err(_) if ancestor.is_symlink() => bail!("--flows must stay inside the repository"),
        Err(error) => return Err(error),
    };
    if !canonical_ancestor.starts_with(canonical_root) {
        bail!("--flows must stay inside the repository");
    }
    Ok(())
}

fn install_all_skills(root: &Path, desired: &str, force: bool) -> bool {
    let mut failed = false;
    // Users may symlink skill directories outside the repository; catalog containment does not apply.
    for (directory, agent_label) in skill::INSTALL_DIRS {
        let relative = Path::new(directory)
            .join(skill::SKILL_NAME)
            .join("SKILL.md");
        if let Err(error) = install_skill(root, &relative, desired, force, agent_label, "the skill")
        {
            eprintln!("error: {}: {error}", slash_path(&relative));
            failed = true;
        }
    }
    failed
}

fn print_next_steps(catalog: &str) {
    println!("\nNext: ask your coding agent to \"document the use-case flows of this repo\".");
    println!(
        "Then: `casemap browse` to see them, `casemap check` to validate them, `casemap diff` after each change."
    );
    println!(
        "\nOptional, so agents always keep flows current, add this line to AGENTS.md and CLAUDE.md:"
    );
    println!(
        "  Use-case flows live in {catalog}/ (casemap skill): update them when behaviour changes and run `casemap check`."
    );
}

struct InitOutcome {
    root: PathBuf,
    catalog: PathBuf,
    display: String,
    failed: bool,
}

fn init_catalog(
    dir: &Path,
    force: bool,
    agent: Option<onboard::Agent>,
    ci: bool,
    hooks: bool,
) -> anyhow::Result<onboard::Outcome> {
    let interactive = interactive_session();
    let agents = if interactive {
        onboard::detect_agents()
    } else {
        Vec::new()
    };
    let chosen_agent = onboard::preflight(agent, interactive, &agents)?;
    let cwd = std::env::current_dir().context("reading the current directory")?;
    let mut initialized = init_core(&cwd, dir, force)?;
    if ci {
        let relative = Path::new(".github/workflows/casemap.yml");
        if let Err(error) = install_ci_workflow(&initialized.root, &initialized.display, force) {
            eprintln!("error: {}: {error}", slash_path(relative));
            initialized.failed = true;
        }
    }
    if hooks {
        initialized.failed |= hook::install_hooks(&initialized.root, &initialized.display, force);
    }
    if chosen_agent.is_none() {
        print_next_steps(&initialized.display);
    }
    if hooks {
        println!("Codex asks you to trust project hooks once: run /hooks in Codex.");
    }
    if initialized.failed {
        return Ok(onboard::Outcome::Exit(ExitCode::FAILURE));
    }
    with_terminal_session(
        &cwd,
        dir,
        onboard::catalog_state(&initialized.catalog),
        agents,
        |mut session| {
            if let Some((agent, program)) = chosen_agent {
                return onboard::launch_and_finish(
                    &mut session,
                    agent,
                    &program,
                    &initialized.root,
                    &initialized.catalog,
                    &initialized.display,
                );
            }
            if interactive && !session.agents.is_empty() {
                return onboard::offer(
                    &mut session,
                    &initialized.root,
                    &initialized.catalog,
                    &initialized.display,
                );
            }
            Ok(onboard::Outcome::Exit(ExitCode::SUCCESS))
        },
    )
}

const CI_WORKFLOW_TEMPLATE: &str = r#"name: casemap

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write

jobs:
  flows:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: Ross-cripto/casemap@v0
        with:
          flows: {catalog}
"#;

fn yaml_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn ci_workflow(catalog: &str) -> String {
    CI_WORKFLOW_TEMPLATE.replace("{catalog}", &yaml_string(catalog))
}

fn install_ci_workflow(root: &Path, catalog: &str, force: bool) -> anyhow::Result<()> {
    let relative = Path::new(".github/workflows/casemap.yml");
    check_skill_symlinks(root, relative)?;
    ensure_inside_root(root, relative)?;
    install_skill(
        root,
        relative,
        &ci_workflow(catalog),
        force,
        "GitHub pull requests",
        "the workflow",
    )?;
    Ok(())
}

fn init_core(cwd: &Path, dir: &Path, force: bool) -> anyhow::Result<InitOutcome> {
    if dir.is_absolute() || dir.has_root() {
        bail!("--flows must stay inside the repository");
    }
    let prefix = match git::locate(cwd)? {
        git::Location::Inside { prefix, .. } => prefix,
        git::Location::NotARepository => {
            println!("note: not a git repository; casemap diff needs git");
            String::new()
        }
    };
    let (root, display) = catalog_display(cwd, dir)?;
    let relative_catalog = normalise_catalog_path(&prefix, dir)?;
    ensure_inside_root(&root, &relative_catalog)?;
    let catalog = root.join(&relative_catalog);
    if catalog.exists() && !catalog.is_dir() {
        bail!("{display} exists and is not a directory");
    }
    let mut failed = false;
    if let Err(error) = create_catalog(&catalog, &display) {
        let path = error.path.strip_prefix(&root).unwrap_or(&error.path);
        eprintln!("error: {}: {}", slash_path(path), error.error);
        failed = true;
    }
    let desired = skill::render(&display);
    failed |= install_all_skills(&root, &desired, force);
    Ok(InitOutcome {
        root,
        catalog,
        display,
        failed,
    })
}

fn watch_frame(
    path: &Path,
    read: io::Result<String>,
    opts: &Options,
    last_good: &mut String,
) -> String {
    let status = match read {
        Ok(source) => match render_source(Some(path), &source, opts) {
            Ok(output) => {
                *last_good = output;
                String::new()
            }
            Err(RenderError::Parse(error)) => format!("parse error, {error}"),
            Err(error @ RenderError::NoBlocks(_)) => error.to_string(),
        },
        Err(error) => format!("cannot read file: {error}"),
    };
    let header = format!("{} · watching for changes", path.display());
    let (dim, bold, reset) = if opts.color {
        ("\x1b[90m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    let mut screen_text = String::from("\x1b[2J\x1b[H");
    screen_text.push_str(&format!("{dim}{header}{reset}\n\n"));
    screen_text.push_str(last_good);
    if !status.is_empty() {
        screen_text.push_str(&format!("\n{bold}{status}{reset}\n"));
    }
    screen_text
}

type FileStamp = Option<(SystemTime, u64)>;

fn file_stamp(path: &Path) -> FileStamp {
    std::fs::metadata(path).ok().map(|metadata| {
        (
            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            metadata.len(),
        )
    })
}

struct Screen {
    last: String,
}

impl Screen {
    fn show(&mut self, frame: &str, out: &mut impl Write) -> bool {
        if frame == self.last {
            return false;
        }
        // A frame that failed to reach the terminal is not remembered, so the next tick retries it.
        if out
            .write_all(frame.as_bytes())
            .and_then(|()| out.flush())
            .is_err()
        {
            return false;
        }
        self.last = frame.to_string();
        true
    }
}

/// The stamp of the last successful read of the watched file.
enum Seen {
    Never,
    Stamp(FileStamp),
}

impl Seen {
    fn matches(&self, stamp: &FileStamp) -> bool {
        matches!(self, Seen::Stamp(seen) if seen == stamp)
    }
}

fn start_watcher(
    dir: &Path,
    sender: mpsc::Sender<notify::Result<notify::Event>>,
) -> notify::Result<notify::RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(sender)?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

enum Wake {
    Events(Receiver<notify::Result<notify::Event>>),
    Poll,
}

fn drain(receiver: &Receiver<notify::Result<notify::Event>>, debounce: Duration, max: Duration) {
    let started = Instant::now();
    while let Some(remaining) = max.checked_sub(started.elapsed()) {
        if receiver.recv_timeout(debounce.min(remaining)).is_err() {
            break;
        }
    }
}

fn wait(wake: &mut Wake) {
    match wake {
        Wake::Events(receiver) => match receiver.recv_timeout(WATCH_HEARTBEAT) {
            Err(RecvTimeoutError::Disconnected) => *wake = Wake::Poll,
            _ => drain(receiver, WATCH_DEBOUNCE, WATCH_MAX_DRAIN),
        },
        Wake::Poll => std::thread::sleep(WATCH_HEARTBEAT),
    }
}

fn watch_file(path: &Path, opts: &Options) -> anyhow::Result<ExitCode> {
    let (sender, receiver) = mpsc::channel();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let watcher = start_watcher(parent, sender);
    let mut wake = match &watcher {
        Ok(_) => Wake::Events(receiver),
        Err(error) => {
            eprintln!("casemap: file events unavailable ({error}); checking every 1s");
            Wake::Poll
        }
    };
    let mut last_seen = Seen::Never;
    let mut last_good = String::new();
    let mut screen = Screen {
        last: String::new(),
    };
    let mut stdout = io::stdout();
    loop {
        let stamp = file_stamp(path);
        if !last_seen.matches(&stamp) {
            let read = std::fs::read_to_string(path);
            if read.is_ok() {
                last_seen = Seen::Stamp(stamp);
            }
            let screen_text = watch_frame(path, read, opts, &mut last_good);
            screen.show(&screen_text, &mut stdout);
        }
        wait(&mut wake);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_string_cases() {
        for (input, expected) in [
            ("docs/flows", "'docs/flows'"),
            ("my flows", "'my flows'"),
            ("it's", "'it''s'"),
            ("true", "'true'"),
        ] {
            assert_eq!(yaml_string(input), expected);
        }
    }

    #[test]
    fn code_root_fallback_only_when_git_is_missing() {
        for (kind, expected) in [
            (io::ErrorKind::NotFound, true),
            (io::ErrorKind::PermissionDenied, false),
            (io::ErrorKind::Other, false),
        ] {
            let error = git::GitError::Spawn {
                error: io::Error::from(kind),
            };
            assert_eq!(falls_back_to_walk(&error), expected);
        }
        let error = git::GitError::Failed {
            args: "rev-parse".into(),
            reason: "broken config".into(),
        };
        assert!(!falls_back_to_walk(&error));
    }

    #[test]
    fn screen_writes_only_changed_frames() {
        let mut screen = Screen {
            last: String::new(),
        };
        let mut output = Vec::new();
        assert!(screen.show("first", &mut output));
        assert!(!screen.show("first", &mut output));
        assert_eq!(output, b"first");
        assert!(screen.show("second", &mut output));
        assert_eq!(output, b"firstsecond");
    }

    #[test]
    fn drain_is_bounded() {
        let (sender, receiver) = mpsc::channel();
        let producer = std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(3) {
                let event = Ok(notify::Event::new(notify::EventKind::Any));
                if sender.send(event).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let started = Instant::now();
        drain(&receiver, WATCH_DEBOUNCE, WATCH_MAX_DRAIN);
        let elapsed = started.elapsed();
        drop(receiver);
        producer.join().unwrap();
        assert!(elapsed < Duration::from_millis(900), "{elapsed:?}");
    }

    #[test]
    fn watch_loop_polls_without_watcher() {
        let started = Instant::now();
        wait(&mut Wake::Poll);
        assert!(started.elapsed() >= Duration::from_millis(900));
    }

    #[test]
    fn watch_frame_keeps_last_good_on_error() {
        let path = Path::new("flow.mmd");
        let opts = Options::default();
        let mut last_good = String::new();
        let good = watch_frame(
            path,
            Ok("graph TD\na[Visible]\n".into()),
            &opts,
            &mut last_good,
        );
        assert_eq!(
            good,
            format!("\x1b[2J\x1b[Hflow.mmd · watching for changes\n\n{last_good}")
        );
        assert!(last_good.contains("Visible"));
        let diagram = last_good.clone();
        let broken = watch_frame(path, Ok("graph TD\na -->".into()), &opts, &mut last_good);
        assert_eq!(last_good, diagram);
        assert!(broken.starts_with(&good));
        assert!(broken.contains("\nparse error, line 2:"));
        let unreadable = watch_frame(
            path,
            Err(io::Error::other("temporarily busy")),
            &opts,
            &mut last_good,
        );
        assert_eq!(last_good, diagram);
        assert_eq!(
            unreadable,
            format!("{good}\ncannot read file: temporarily busy\n")
        );
    }

    #[test]
    fn skill_commands_parse() {
        let mut checked = 0;
        for span in skill::SKILL_MD.split('`').skip(1).step_by(2) {
            if span.starts_with("casemap ") {
                checked += 1;
                let command = span.replace("<name>", "x").replace("[REF]", "main");
                if let Err(error) = Cli::try_parse_from(command.split_whitespace()) {
                    assert!(!error.use_stderr(), "{span}: {error}");
                }
            }
        }
        assert!(checked >= 5);
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_like_before() {
        for file in ["./diff", "-", "help"] {
            let cli = Cli::try_parse_from(["casemap", file]).unwrap();
            assert_eq!(cli.file.as_deref(), Some(Path::new(file)));
            assert!(cli.command.is_none());
        }
        for args in [
            ["casemap", "--watch", "diff"],
            ["casemap", "diff", "--watch"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.watch);
            assert!(matches!(
                cli.command,
                Some(Commands::Diff {
                    rev: None,
                    strict: false,
                    format: DiffFormat::Text
                })
            ));
        }
        let cli = Cli::try_parse_from(["casemap", "--dir", "LR", "--dir", "TD"]).unwrap();
        assert_eq!(cli.dir, Some(Direction::TopDown));
        assert!(
            Cli::try_parse_from(["casemap", "--ascii", "--ascii"])
                .unwrap()
                .ascii
        );
    }
}
