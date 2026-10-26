//! strata CLI: adopt, validate, render and compare a catalog of use-case flows.

use std::io::{self, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, bail};
use clap::{CommandFactory, Parser, Subcommand};
use notify::{RecursiveMode, Watcher};
use strata::flows::Flow;
use strata::markdown::is_markdown_path;
use strata::{Direction, Options, ParseError, anchors, check, git, skill};

const DEFAULT_FLOWS_DIR: &str = skill::DEFAULT_CATALOG;
const DEFAULT_REF: &str = "HEAD";
const WATCH_HEARTBEAT: Duration = Duration::from_secs(1);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);
const WATCH_MAX_DRAIN: Duration = Duration::from_millis(500);

#[derive(Debug, Parser)]
#[command(
    name = "strata",
    version,
    about = "The living map of your app's use cases: one Mermaid flow per use case in docs/flows/, written by your coding agent, checked and rendered in the terminal.",
    after_help = "Quick start:
  strata init           create docs/flows/ and install the agent skill
  (ask your agent)      \"document the use-case flows of this repo\"
  strata flows          see them
  strata check          validate them
  strata diff main      what a branch did to them",
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

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create the flow catalog and install the strata-flows agent skill.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// List the flow catalog, or render one flow.
    Flows {
        name: Option<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
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
    /// Print the strata-flows agent skill without its front matter.
    Guide,
    /// Generate shell completions.
    Completions { shell: clap_complete::Shell },
}

fn parse_direction(value: &str) -> Result<Direction, String> {
    Direction::parse(value).ok_or_else(|| "expects one of TD, BT, LR, RL".into())
}

fn main() -> ExitCode {
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
            eprintln!("strata: {error:#}");
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
    let directory = cli
        .flows
        .as_deref()
        .unwrap_or_else(|| Path::new(DEFAULT_FLOWS_DIR));
    match cli.command {
        Some(Commands::Diff { rev, strict }) => diff_catalog(
            directory,
            rev.as_deref().unwrap_or(DEFAULT_REF),
            strict,
            &opts,
        ),
        Some(Commands::Which { paths, format }) => which_flows(directory, &paths, format),
        Some(Commands::Init { force }) => init_catalog(directory, force),
        Some(Commands::Flows { name, format }) => {
            show_flows(directory, name.as_deref(), format, &opts)
        }
        Some(Commands::Check { strict, format }) => check_catalog(directory, strict, format, color),
        Some(Commands::Guide) => {
            print_guide(directory);
            Ok(ExitCode::SUCCESS)
        }
        Some(Commands::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "strata", &mut io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        None => render_file(cli.file.as_deref(), cli.watch, &opts),
    }
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
        return Ok(strata::render_str(src, opts)?);
    };
    if strata::markdown::mermaid_blocks(src).is_empty() {
        return Err(RenderError::NoBlocks(path.display().to_string()));
    }
    Ok(strata::render_markdown(src, opts)?)
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
    Ok(strata::flows::load_dir(dir)?)
}

fn diff_catalog(dir: &Path, rev: &str, strict: bool, opts: &Options) -> anyhow::Result<ExitCode> {
    let new = load_catalog(dir)?;
    let base = strata::git::merge_base(dir, rev)?;
    let old = strata::git::files_at(dir, &base)?
        .into_iter()
        .map(|(path, text)| strata::flows::parse_flow(&path, &text))
        .collect::<Vec<_>>();
    let changes = strata::flows::compare(&old, &new);
    let stale = if new
        .iter()
        .any(|flow| !anchors::flow_anchors(flow).0.is_empty())
    {
        let (root, _, prefix) = code_root(dir)?;
        let changed = git::changed_files(&root, &base)?;
        strata::flows::stale_flows(&new, &prefix, &changed)
    } else {
        Vec::new()
    };
    print!(
        "{}",
        strata::flows::render_report(&changes, &stale, rev, &base, opts)
    );
    if changes
        .iter()
        .any(|change| matches!(change, strata::flows::FlowChange::Broken { .. }))
        || (strict && !stale.is_empty())
    {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
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
    let flows = load_catalog(dir)?;
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
    let Some(name) = name else {
        if flows.is_empty() {
            println!("no flows in {}", dir.display());
        } else {
            print!("{}", strata::flows::render_list(&flows));
        }
        return Ok(ExitCode::SUCCESS);
    };
    let Some(flow) = flows.iter().find(|flow| flow.name == name) else {
        let available: Vec<&str> = flows.iter().map(|flow| flow.name.as_str()).collect();
        bail!("no flow named {name}; available: {}", available.join(", "));
    };
    let diagram = match strata::flows::diagram(flow) {
        Some(Ok(diagram)) => diagram,
        Some(Err(error)) => bail!("{}: {error}", flow.path),
        None => bail!("{}: no mermaid block", flow.path),
    };
    print!("{}", strata::render::bold_line(&flow.title, opts.color));
    if let Some(description) = &flow.description {
        println!("{description}");
    }
    println!();
    print!("{}", strata::render_diagram(&diagram, opts));
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
    root: PathBuf,
    source: FileSource,
    files: Option<Vec<String>>,
    error: Option<anyhow::Error>,
}

impl LazyCodeIndex {
    fn new(catalog_dir: &Path) -> anyhow::Result<Self> {
        let (root, source, _) = code_root(catalog_dir)?;
        Ok(Self {
            root,
            source,
            files: None,
            error: None,
        })
    }

    fn files(&mut self) -> anyhow::Result<&[String]> {
        if self.files.is_none() {
            self.files = Some(code_files(&self.root, self.source)?);
        }
        Ok(self.files.as_deref().unwrap_or_default())
    }

    fn has_match(&mut self, pattern: &str) -> bool {
        if self.error.is_some() {
            return true;
        }
        let patterns = [pattern.to_owned()];
        match self.files() {
            Ok(files) => files
                .iter()
                .any(|path| anchors::any_pattern_matches(&patterns, path)),
            Err(error) => {
                self.error = Some(error);
                true
            }
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
    flows: Vec<strata::flows::AnchoredFlow<'a>>,
}

fn which_flows(dir: &Path, paths: &[PathBuf], format: Format) -> anyhow::Result<ExitCode> {
    let flows = load_catalog(dir)?;
    let mut index = LazyCodeIndex::new(dir)?;
    let root = index.root.clone();
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
            flows: strata::flows::anchored_flows(&flows, &files),
        });
    }
    match format {
        Format::Text => print!("{}", render_which_text(&results)),
        Format::Json => print_which_json(dir, &root, &results)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn file_is_under_directory(file: &str, directory: &str) -> bool {
    directory.is_empty()
        || file
            .strip_prefix(directory)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn which_annotation(flow: &strata::flows::AnchoredFlow<'_>) -> String {
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

fn check_catalog(
    dir: &Path,
    strict: bool,
    format: Format,
    color: bool,
) -> anyhow::Result<ExitCode> {
    require_catalog(dir)?;
    let mut index = None;
    // the catalog error is more fundamental than an index error
    let report = check::check_catalog_with(dir, &mut |pattern| match index
        .get_or_insert_with(|| LazyCodeIndex::new(dir))
    {
        Ok(index) => index.has_match(pattern),
        Err(_) => true,
    })?;
    if let Some(index) = index {
        let mut index = index?;
        if let Some(error) = index.take_error() {
            return Err(error);
        }
    }
    match format {
        Format::Text => print_check_text(dir, &report, color),
        Format::Json => print_check_json(dir, &report),
    }
    Ok(check_exit_code(&report, strict))
}

fn print_check_text(dir: &Path, report: &check::Report, color: bool) {
    for problem in &report.problems {
        println!(
            "{}: {}: {} [{}]",
            problem_location(dir, problem),
            coloured_severity(problem.code.severity(), color),
            problem.message,
            problem.code.as_str()
        );
    }
    println!("{}", check_summary(report));
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

fn catalog_write_error(path: &Path, error: io::Error) -> strata::flows::CatalogError {
    strata::flows::CatalogError {
        path: path.to_path_buf(),
        error,
    }
}

fn create_catalog(dir: &Path, display: &str) -> Result<(), strata::flows::CatalogError> {
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
    name.push(".strata-tmp");
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
            println!("kept {display} (differs from this strata version; use --force to update)")
        }
        SkillAction::Update => {
            write_skill(&path, desired)?;
            println!("updated {display}");
        }
    }
    if git::is_ignored(root, relative) {
        println!("warning: {display} is gitignored; teammates will not get the skill");
    }
    Ok(())
}

fn locate_install_root(cwd: &Path) -> anyhow::Result<(PathBuf, String)> {
    match git::locate(cwd)? {
        git::Location::Inside { root, prefix } => Ok((root, prefix)),
        git::Location::NotARepository => {
            println!("note: not a git repository; strata diff needs git");
            Ok((cwd.to_path_buf(), String::new()))
        }
    }
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
        if let Err(error) = install_skill(root, &relative, desired, force, agent_label) {
            eprintln!("error: {}: {error}", slash_path(&relative));
            failed = true;
        }
    }
    failed
}

fn print_next_steps(catalog: &str) {
    println!("\nNext: ask your coding agent to \"document the use-case flows of this repo\".");
    println!(
        "Then: `strata flows` to see them, `strata check` to validate them, `strata diff` after each change."
    );
    println!(
        "\nOptional, so agents always keep flows current, add this line to AGENTS.md and CLAUDE.md:"
    );
    println!(
        "  Use-case flows live in {catalog}/ (strata-flows skill): update them when behaviour changes and run `strata check`."
    );
}

fn init_catalog(dir: &Path, force: bool) -> anyhow::Result<ExitCode> {
    if dir.is_absolute() || dir.has_root() {
        bail!("--flows must stay inside the repository");
    }
    let cwd = std::env::current_dir().context("reading the current directory")?;
    let (root, prefix) = locate_install_root(&cwd)?;
    let relative_catalog = normalise_catalog_path(&prefix, dir)?;
    ensure_inside_root(&root, &relative_catalog)?;
    let catalog = root.join(&relative_catalog);
    let display = catalog_path(&relative_catalog);
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
    print_next_steps(&display);
    if failed {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
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
            eprintln!("strata: file events unavailable ({error}); checking every 1s");
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
            if span.starts_with("strata ") {
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
            let cli = Cli::try_parse_from(["strata", file]).unwrap();
            assert_eq!(cli.file.as_deref(), Some(Path::new(file)));
            assert!(cli.command.is_none());
        }
        for args in [["strata", "--watch", "diff"], ["strata", "diff", "--watch"]] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(cli.watch);
            assert!(matches!(
                cli.command,
                Some(Commands::Diff {
                    rev: None,
                    strict: false
                })
            ));
        }
        let cli = Cli::try_parse_from(["strata", "--dir", "LR", "--dir", "TD"]).unwrap();
        assert_eq!(cli.dir, Some(Direction::TopDown));
        assert!(
            Cli::try_parse_from(["strata", "--ascii", "--ascii"])
                .unwrap()
                .ascii
        );
    }
}
