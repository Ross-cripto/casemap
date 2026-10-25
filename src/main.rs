//! strata CLI — render a Mermaid flowchart once, or keep re-rendering it as
//! the file changes so the diagram can live in a side pane while you work.

use std::io::{IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use strata::{Direction, Options};

const USAGE: &str = "\
strata — render Mermaid flowcharts in the terminal

USAGE:
    strata [OPTIONS] [FILE]

Reads FILE (or stdin when FILE is omitted or '-') and prints the diagram.

OPTIONS:
    -w, --watch           Re-render whenever FILE changes (Ctrl-C to quit)
    --dir <TD|BT|LR|RL>   Override the flowchart direction
    --ascii               Use plain ASCII instead of box-drawing characters
    --color               Force ANSI colour
    --no-color            Disable ANSI colour (also respects NO_COLOR)
    -h, --help            Show this help
";

fn main() -> ExitCode {
    let mut file: Option<String> = None;
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
            "--dir" => match args.next().as_deref().and_then(Direction::parse) {
                Some(d) => opts.direction = Some(d),
                None => return fail("--dir expects one of TD, BT, LR, RL"),
            },
            _ if arg.starts_with('-') && arg != "-" => {
                return fail(&format!("unknown option {arg}\n\n{USAGE}"))
            }
            _ => file = Some(arg),
        }
    }
    opts.color = color.unwrap_or_else(|| {
        std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
    });

    if watch {
        return match file.as_deref() {
            Some(path) if path != "-" => watch_file(Path::new(path), &opts),
            _ => fail("--watch needs a file path"),
        };
    }

    let source = match file.as_deref() {
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
    match strata::render_str(&source, &opts) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&e.to_string()),
    }
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
                Ok(src) => match strata::render_str(&src, opts) {
                    Ok(out) => {
                        last_good = out;
                        String::new()
                    }
                    Err(e) => format!("parse error, {e}"),
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
