//! strata CLI — render a Mermaid flowchart once, or keep re-rendering it as
//! the file changes so the diagram can live in a side pane while you work.

use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, bail};
use clap::{CommandFactory, Parser, Subcommand};
use notify::{RecursiveMode, Watcher};
use strata::flows::Flow;
use strata::markdown::is_markdown_path;
use strata::{Direction, Options};

const DEFAULT_FLOWS_DIR: &str = "docs/flows";
const DEFAULT_REF: &str = "HEAD";
const WATCH_HEARTBEAT: Duration = Duration::from_secs(1);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);

#[derive(Debug, Parser)]
#[command(
    name = "strata",
    version,
    about,
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

#[derive(Debug, Subcommand)]
enum Commands {
    /// Show catalog changes since REF (default HEAD), including uncommitted edits.
    Diff { rev: Option<String> },
    /// List the flow catalog, or render one flow.
    Flows { name: Option<String> },
    /// Print the flow-catalog convention for AGENTS.md or CLAUDE.md.
    Guide,
    /// Generate shell completions.
    Completions { shell: clap_complete::Shell },
}

fn parse_direction(value: &str) -> Result<Direction, String> {
    Direction::parse(value).ok_or_else(|| "expects one of TD, BT, LR, RL".into())
}

const GUIDE: &str = r"# Flow catalog

Every user-facing use case of this project has one file in docs/flows/,
named after the use case (docs/flows/checkout.md):

    # Checkout

    A customer pays for the cart and gets a confirmation.

    ```mermaid
    flowchart TD
        cart([Customer opens cart]) --> pay{Payment ok?}
        pay -->|yes| confirm[Order confirmed]
        pay -->|no| error[Show payment error]
    ```

Rules for agents:
- When a task changes how a use case behaves, update its flow file in the
  same change. Add a file for a new use case; delete the file of a removed one.
- Describe the flow as it is after your change. Do not mark what changed:
  `strata diff` computes that from git.
- Keep node ids stable. Rename a step by changing its label, not its id;
  a new id reads as one step removed and another added.
- One mermaid flowchart per file. Name steps after behaviour a user would
  recognise, not after functions or classes.
- Check the result with `strata diff` (uncommitted changes) or
  `strata diff main` (the whole branch).
";

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
    let color = if cli.no_color {
        false
    } else if cli.color {
        true
    } else {
        io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
    };
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
        Some(Commands::Diff { rev }) => {
            diff_catalog(directory, rev.as_deref().unwrap_or(DEFAULT_REF), &opts)
        }
        Some(Commands::Flows { name }) => show_flows(directory, name.as_deref(), &opts),
        Some(Commands::Guide) => {
            print!(
                "{}",
                GUIDE.replace(DEFAULT_FLOWS_DIR, &directory.to_string_lossy())
            );
            Ok(ExitCode::SUCCESS)
        }
        Some(Commands::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "strata", &mut io::stdout());
            Ok(ExitCode::SUCCESS)
        }
        None => {
            let file = cli.file.as_deref();
            if cli.watch {
                return match file {
                    Some(path) if path != Path::new("-") => watch_file(path, &opts),
                    _ => bail!("--watch needs a file path"),
                };
            }
            let source = match file {
                None => read_stdin()?,
                Some(path) if path == Path::new("-") => read_stdin()?,
                Some(path) => {
                    std::fs::read_to_string(path).with_context(|| path.display().to_string())?
                }
            };
            print!("{}", render_source(file, &source, &opts)?);
            Ok(ExitCode::SUCCESS)
        }
    }
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
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    NoBlocks(String),
}

fn render_source(path: Option<&Path>, src: &str, opts: &Options) -> Result<String, RenderError> {
    let Some(path) = path.filter(|path| is_markdown_path(path)) else {
        return strata::render_str(src, opts)
            .map_err(|error| RenderError::Parse(error.to_string()));
    };
    if strata::markdown::mermaid_blocks(src).is_empty() {
        return Err(RenderError::NoBlocks(format!(
            "{}: no mermaid blocks",
            path.display()
        )));
    }
    strata::render_markdown(src, opts).map_err(|error| RenderError::Parse(error.to_string()))
}

fn load_catalog(dir: &Path) -> anyhow::Result<Vec<Flow>> {
    if !dir.is_dir() {
        bail!(
            "no flow catalog at {} (create it or pass --flows <dir>)",
            dir.display()
        );
    }
    Ok(strata::flows::load_dir(dir)?)
}

fn diff_catalog(dir: &Path, rev: &str, opts: &Options) -> anyhow::Result<ExitCode> {
    let new = load_catalog(dir)?;
    let base = strata::git::merge_base(dir, rev)?;
    let old = strata::git::files_at(dir, &base)?
        .into_iter()
        .map(|(path, text)| strata::flows::parse_flow(&path, &text))
        .collect::<Vec<_>>();
    let changes = strata::flows::compare(&old, &new);
    print!(
        "{}",
        strata::flows::render_report(&changes, rev, &base, opts)
    );
    if changes
        .iter()
        .any(|change| matches!(change, strata::flows::FlowChange::Broken { .. }))
    {
        Ok(ExitCode::FAILURE)
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn show_flows(dir: &Path, name: Option<&str>, opts: &Options) -> anyhow::Result<ExitCode> {
    let flows = load_catalog(dir)?;
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
    Ok(ExitCode::SUCCESS)
}

fn frame(path: &Path, read: io::Result<String>, opts: &Options, last_good: &mut String) -> String {
    let status = match read {
        Ok(source) => match render_source(Some(path), &source, opts) {
            Ok(output) => {
                *last_good = output;
                String::new()
            }
            Err(RenderError::Parse(message)) => format!("parse error, {message}"),
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
    let mut frame = String::from("\x1b[2J\x1b[H");
    frame.push_str(&format!("{dim}{header}{reset}\n\n"));
    frame.push_str(last_good);
    if !status.is_empty() {
        frame.push_str(&format!("\n{bold}{status}{reset}\n"));
    }
    frame
}

fn watch_file(path: &Path, opts: &Options) -> anyhow::Result<ExitCode> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
    let mut last_seen = None;
    let mut last_good = String::new();
    let mut stdout = io::stdout();
    loop {
        let stamp = std::fs::metadata(path).ok().map(|metadata| {
            (
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                metadata.len(),
            )
        });
        if last_seen != Some(stamp) {
            let read = std::fs::read_to_string(path);
            if read.is_ok() {
                last_seen = Some(stamp);
            }
            let frame = frame(path, read, opts, &mut last_good);
            let _ = stdout.write_all(frame.as_bytes());
            let _ = stdout.flush();
        }
        let _ = receiver.recv_timeout(WATCH_HEARTBEAT);
        while receiver.recv_timeout(WATCH_DEBOUNCE).is_ok() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_frame_keeps_last_good_on_error() {
        let path = Path::new("flow.mmd");
        let opts = Options::default();
        let mut last_good = String::new();
        let good = frame(
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
        let broken = frame(path, Ok("graph TD\na -->".into()), &opts, &mut last_good);
        assert_eq!(last_good, diagram);
        assert!(broken.starts_with(&good));
        assert!(broken.contains("\nparse error, line 2:"));
        let unreadable = frame(
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
            assert!(matches!(cli.command, Some(Commands::Diff { rev: None })));
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
