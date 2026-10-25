//! strata CLI — render a Mermaid flowchart once, or keep re-rendering it as
//! the file changes so the diagram can live in a side pane while you work.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use strata::flows::Flow;
use strata::markdown::is_markdown_path;
use strata::{Direction, Options};

const DEFAULT_FLOWS_DIR: &str = "docs/flows";
const DEFAULT_REF: &str = "HEAD";
const MAX_CATALOG_POSITIONALS: usize = 2; // command word + one argument

enum CatalogCommand {
    Diff,
    Flows,
    Guide,
}

impl CatalogCommand {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "diff" => Some(Self::Diff),
            "flows" => Some(Self::Flows),
            "guide" => Some(Self::Guide),
            _ => None,
        }
    }
}

const USAGE: &str = "\
strata — render Mermaid flowcharts in the terminal

USAGE:
    strata [OPTIONS] [FILE]      Render FILE (.mmd, or every mermaid block of a .md); stdin when omitted or '-'
    strata diff [REF]            Show how the flow catalog changed since REF (default HEAD), uncommitted edits included
    strata flows [NAME]          List the flow catalog, or render one flow
    strata guide                 Print the flow-catalog convention to paste into AGENTS.md or CLAUDE.md

OPTIONS:
    -w, --watch           Re-render whenever FILE changes (Ctrl-C to quit)
    --flows <DIR>         Flow catalog directory (default docs/flows)
    --dir <TD|BT|LR|RL>   Override the flowchart direction
    --ascii               Use plain ASCII instead of box-drawing characters
    --color               Force ANSI colour
    --no-color            Disable ANSI colour (also respects NO_COLOR)
    -h, --help            Show this help
";

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
    let mut positionals: Vec<String> = Vec::new();
    let mut flows_dir: Option<String> = None;
    let mut watch = false;
    let mut color: Option<bool> = None;
    let mut opts = Options::default();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-w" | "--watch" => watch = true,
            "--ascii" => opts.ascii = true,
            "--color" => color = Some(true),
            "--no-color" => color = Some(false),
            "--flows" => match args.next() {
                Some(dir) => flows_dir = Some(dir),
                None => return fail("--flows expects a directory"),
            },
            "--dir" => match args.next().as_deref().and_then(Direction::parse) {
                Some(d) => opts.direction = Some(d),
                None => return fail("--dir expects one of TD, BT, LR, RL"),
            },
            _ if arg.starts_with('-') && arg != "-" => {
                return fail(&format!("unknown option {arg}\n\n{USAGE}"))
            }
            _ => positionals.push(arg),
        }
    }
    opts.color = color.unwrap_or_else(|| {
        std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
    });

    if let Some(command) = positionals
        .first()
        .and_then(|word| CatalogCommand::parse(word))
    {
        if watch {
            return fail("--watch only applies to rendering a file");
        }
        if positionals.len() > MAX_CATALOG_POSITIONALS {
            return fail(USAGE);
        }
        let argument = positionals.get(1).map(String::as_str);
        let directory = Path::new(flows_dir.as_deref().unwrap_or(DEFAULT_FLOWS_DIR));
        return run_catalog_command(command, argument, directory, &opts);
    }
    if positionals.len() > 1 {
        return fail(USAGE);
    }
    let file = positionals.first().map(String::as_str);
    if watch {
        return match file {
            Some(path) if path != "-" => watch_file(Path::new(path), &opts),
            _ => fail("--watch needs a file path"),
        };
    }

    let source = match file {
        None | Some("-") => {
            let mut s = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                return fail(&format!("reading stdin: {e}"));
            }
            s
        }
        Some(path) => match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => return fail(&format!("{path}: {e}")),
        },
    };
    match render_source(file.map(Path::new), &source, &opts) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
}

fn run_catalog_command(
    command: CatalogCommand,
    argument: Option<&str>,
    directory: &Path,
    opts: &Options,
) -> ExitCode {
    match command {
        CatalogCommand::Diff => diff_catalog(directory, argument.unwrap_or(DEFAULT_REF), opts),
        CatalogCommand::Flows => show_flows(directory, argument, opts),
        CatalogCommand::Guide => {
            if argument.is_some() {
                return fail(USAGE);
            }
            print!(
                "{}",
                GUIDE.replace(DEFAULT_FLOWS_DIR, &directory.to_string_lossy())
            );
            ExitCode::SUCCESS
        }
    }
}

#[derive(Debug)]
enum RenderError {
    Parse(String),
    NoBlocks(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) | Self::NoBlocks(message) => formatter.write_str(message),
        }
    }
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

fn check_catalog(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!(
            "no flow catalog at {} (create it or pass --flows <dir>)",
            dir.display()
        ));
    }
    Ok(())
}

fn load_catalog(dir: &Path) -> Result<Vec<Flow>, String> {
    check_catalog(dir)?;
    strata::flows::load_dir(dir).map_err(|error| error.to_string())
}

fn diff_catalog(dir: &Path, rev: &str, opts: &Options) -> ExitCode {
    let new = match load_catalog(dir) {
        Ok(flows) => flows,
        Err(error) => return fail(&error),
    };
    let base = match strata::git::merge_base(dir, rev) {
        Ok(base) => base,
        Err(error) => return fail(&error.to_string()),
    };
    let old = match strata::git::files_at(dir, &base) {
        Ok(files) => files
            .into_iter()
            .map(|(path, text)| strata::flows::parse_flow(&path, &text))
            .collect::<Vec<_>>(),
        Err(error) => return fail(&error.to_string()),
    };
    let changes = strata::flows::compare(&old, &new);
    print!(
        "{}",
        strata::flows::render_report(&changes, rev, &base, opts)
    );
    if changes
        .iter()
        .any(|change| matches!(change, strata::flows::FlowChange::Broken { .. }))
    {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn show_flows(dir: &Path, name: Option<&str>, opts: &Options) -> ExitCode {
    let flows = match load_catalog(dir) {
        Ok(flows) => flows,
        Err(error) => return fail(&error),
    };
    let Some(name) = name else {
        if flows.is_empty() {
            println!("no flows in {}", dir.display());
        } else {
            print!("{}", strata::flows::render_list(&flows));
        }
        return ExitCode::SUCCESS;
    };
    let Some(flow) = flows.iter().find(|flow| flow.name == name) else {
        let available: Vec<&str> = flows.iter().map(|flow| flow.name.as_str()).collect();
        return fail(&format!(
            "no flow named {name}; available: {}",
            available.join(", ")
        ));
    };
    let diagram = match strata::flows::diagram(flow) {
        Some(Ok(diagram)) => diagram,
        Some(Err(error)) => return fail(&format!("{}: {error}", flow.path)),
        None => return fail(&format!("{}: no mermaid block", flow.path)),
    };
    print!("{}", strata::render::bold_line(&flow.title, opts.color));
    if let Some(description) = &flow.description {
        println!("{description}");
    }
    println!();
    print!("{}", strata::render_diagram(&diagram, opts));
    ExitCode::SUCCESS
}

/// Polls the file and redraws on change. Keeps the last good diagram on
/// screen when an edit is temporarily invalid, with the error underneath.
fn watch_file(path: &Path, opts: &Options) -> ExitCode {
    let mut last_seen: Option<(SystemTime, u64)> = None;
    let mut last_good = String::new();
    let mut stdout = std::io::stdout();
    loop {
        let stamp = std::fs::metadata(path)
            .ok()
            .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
        if stamp != last_seen {
            last_seen = stamp;
            let status = match std::fs::read_to_string(path) {
                Ok(src) => match render_source(Some(path), &src, opts) {
                    Ok(out) => {
                        last_good = out;
                        String::new()
                    }
                    Err(RenderError::Parse(message)) => format!("parse error, {message}"),
                    Err(error @ RenderError::NoBlocks(_)) => error.to_string(),
                },
                Err(e) => format!("cannot read file: {e}"),
            };
            let header = format!("{} · watching for changes", path.display());
            let (dim, bold, reset) = if opts.color {
                ("\x1b[90m", "\x1b[1m", "\x1b[0m")
            } else {
                ("", "", "")
            };
            let mut frame = String::from("\x1b[2J\x1b[H");
            frame.push_str(&format!("{dim}{header}{reset}\n\n"));
            frame.push_str(&last_good);
            if !status.is_empty() {
                frame.push_str(&format!("\n{bold}{status}{reset}\n"));
            }
            let _ = stdout.write_all(frame.as_bytes());
            let _ = stdout.flush();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("strata: {msg}");
    ExitCode::FAILURE
}
