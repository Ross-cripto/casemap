mod ansi;
pub(crate) mod app;
mod layout;
mod ui;

use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::bail;
use ratatui::crossterm::event::{self, Event};
use strata::flows::{self, Flow, FlowChange};
use strata::{Diagram, Options, anchors, check, diff, git};

use crate::{BaseChanges, DEFAULT_REF, WATCH_DEBOUNCE, WATCH_HEARTBEAT, Wake};
use app::{Action, App, BaseInfo, DiagramView, Entry, Loaded, Marker};

const KEY_POLL: Duration = Duration::from_millis(250);

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

pub(crate) fn run(dir: &Path, reference: Option<&str>, opts: &Options) -> anyhow::Result<ExitCode> {
    crate::require_catalog(dir)?;
    if let Some(reference) = reference {
        git::merge_base(dir, reference)?;
    }
    if !io::stdout().is_terminal() {
        bail!("strata browse needs a terminal");
    }
    let mut base = BrowserBase {
        reference: reference.unwrap_or(DEFAULT_REF).into(),
        data: None,
        initial: true,
        explicit: reference.is_some(),
    };
    let loaded = load(dir, &mut base, opts)?;
    let mut app = App::new(loaded, opts.ascii, opts.color);
    let (sender, receiver) = mpsc::channel();
    let watcher = crate::start_watcher(dir, sender);
    let mut wake = match &watcher {
        Ok(_) => Wake::Events(receiver),
        Err(error) => {
            app.status = Some(watcher_message(error));
            Wake::Poll
        }
    };
    let mut stamp = catalog_stamp(dir)?;
    let mut heartbeat = Instant::now();
    let mut pending = None;
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init()?;
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let mut reload = false;
        if event::poll(KEY_POLL)? {
            match event::read()? {
                Event::Key(key) => match app.handle(key) {
                    Action::Quit => return Ok(ExitCode::SUCCESS),
                    Action::Reload => reload = true,
                    Action::None => {}
                },
                Event::Resize(cols, rows) => app.resize(cols, rows),
                _ => {}
            }
        }
        if let Wake::Events(receiver) = &wake {
            let mut failure = None;
            for message in receiver.try_iter() {
                match message {
                    Ok(_) => pending = Some(Instant::now()),
                    Err(error) => failure = Some(watcher_message(&error)),
                }
            }
            if let Some(message) = failure {
                app.status = Some(message);
                wake = Wake::Poll;
            }
        }
        if pending.is_some_and(|last: Instant| last.elapsed() >= WATCH_DEBOUNCE) {
            pending = None;
            reload = true;
        }
        if matches!(wake, Wake::Poll) && heartbeat.elapsed() >= WATCH_HEARTBEAT {
            heartbeat = Instant::now();
            match catalog_stamp(dir) {
                Ok(current) if current != stamp => {
                    stamp = current;
                    reload = true;
                }
                Ok(_) => {}
                Err(error) => app.status = Some(format!("catalog: {error:#}")),
            }
        }
        if reload {
            match load(dir, &mut base, opts) {
                Ok(loaded) => app.apply(loaded),
                Err(error) => app.status = Some(format!("reload failed: {error:#}")),
            }
        }
    }
}

fn watcher_message(error: &notify::Error) -> String {
    format!("file events unavailable ({error}); checking every 1s")
}

#[derive(Debug, PartialEq, Eq)]
struct CatalogStamp(Vec<(String, Option<SystemTime>, u64)>);

fn catalog_stamp(dir: &Path) -> anyhow::Result<CatalogStamp> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = entry.metadata();
        let (modified, length) = match metadata {
            Ok(metadata) => (metadata.modified().ok(), metadata.len()),
            Err(_) => (None, 0),
        };
        files.push((name, modified, length));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(CatalogStamp(files))
}

struct BrowserBase {
    reference: String,
    data: Option<BaseChanges>,
    initial: bool,
    explicit: bool,
}

fn load(dir: &Path, base: &mut BrowserBase, opts: &Options) -> anyhow::Result<Loaded> {
    let current = crate::load_catalog(dir)?;
    let report = crate::check_report(dir)?;
    let mut status = None;
    match crate::base_changes(dir, &base.reference, &current) {
        Ok(data) => base.data = Some(data),
        Err(error) if base.initial && base.explicit => return Err(error),
        Err(error) if !base.initial => status = Some(format!("git reload failed: {error:#}")),
        Err(_) => {}
    }
    base.initial = false;
    let mut loaded = build_loaded(
        dir,
        current,
        report,
        &base.reference,
        base.data.as_ref(),
        opts,
    );
    loaded.status = status;
    Ok(loaded)
}

fn build_loaded(
    dir: &Path,
    current: Vec<Flow>,
    report: check::Report,
    reference: &str,
    base: Option<&BaseChanges>,
    opts: &Options,
) -> Loaded {
    let mut entries = Vec::new();
    for flow in &current {
        entries.push(build_entry(dir, flow, &report, base, opts));
    }
    if let Some(base) = base {
        for flow in &base.old {
            if !current.iter().any(|current| current.path == flow.path) {
                entries.push(build_entry(dir, flow, &report, Some(base), opts));
            }
        }
    }
    Loaded {
        flows: entries,
        base: base.map(|base| BaseInfo {
            reference: reference.into(),
            sha: base.base.clone(),
        }),
        catalog_problems: report
            .problems
            .iter()
            .filter(|problem| problem.file.is_none())
            .count(),
        status: None,
    }
}

fn changed_flow(change: &FlowChange) -> &Flow {
    match change {
        FlowChange::Modified { flow, .. }
        | FlowChange::New { flow, .. }
        | FlowChange::Removed { flow }
        | FlowChange::Unchanged { flow }
        | FlowChange::Broken { flow, .. } => flow,
    }
}

fn build_entry(
    dir: &Path,
    flow: &Flow,
    report: &check::Report,
    base: Option<&BaseChanges>,
    opts: &Options,
) -> Entry {
    let normal = normal_view(flow, opts);
    let change = base.and_then(|base| {
        base.changes
            .iter()
            .find(|change| changed_flow(change).path == flow.path)
    });
    let stale = base.and_then(|base| base.stale.iter().find(|stale| stale.flow.path == flow.path));
    let previous = base.and_then(|base| base.old.iter().find(|old| old.path == flow.path));
    let (difference, change_marker) = diff_view(flow, previous, &normal, change, opts);
    let problems: Vec<_> = report
        .problems
        .iter()
        .filter(|problem| problem.file.as_deref() == Some(&flow.path))
        .collect();
    let check_errors = problems
        .iter()
        .any(|problem| problem.code.severity() == check::Severity::Error);
    let broken = matches!(flows::diagram(flow), Some(Err(_)));
    Entry {
        flow: flow.clone(),
        path: crate::catalog_file_path(dir, &flow.path),
        marker: Marker::choose(broken, check_errors, stale.is_some(), change_marker),
        normal,
        difference,
        problems: problems.into_iter().map(format_problem).collect(),
        footer: anchors::render_footer(flow)
            .lines()
            .skip(2)
            .map(str::to_owned)
            .collect(),
        stale: stale
            .map(|stale| {
                stale
                    .hits
                    .iter()
                    .map(|hit| format!("{}: {}", hit.target.label(), hit.files.join(", ")))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn format_problem(problem: &check::Problem) -> String {
    let location = problem
        .line
        .map(|line| format!("line {line}: "))
        .unwrap_or_default();
    format!(
        "{location}{}: {} [{}]",
        crate::severity_text(problem.code.severity()),
        problem.message,
        problem.code.as_str()
    )
}

fn normal_view(flow: &Flow, opts: &Options) -> DiagramView {
    let rendered = match flows::diagram(flow) {
        Some(Ok(diagram)) => strata::render_diagram(&diagram, opts),
        Some(Err(error)) => format!("parse error, {error}"),
        None => "no mermaid block".into(),
    };
    DiagramView {
        text: ansi::ansi_to_text(&rendered),
        summary: "unchanged since base".into(),
        details: Vec::new(),
    }
}

fn diff_view(
    flow: &Flow,
    previous: Option<&Flow>,
    normal: &DiagramView,
    change: Option<&FlowChange>,
    opts: &Options,
) -> (DiagramView, Marker) {
    let Some(change) = change else {
        return (normal.clone(), Marker::Same);
    };
    match change {
        FlowChange::Modified { diff, .. } => (
            render_difference(diff, diff.summary(), opts),
            Marker::Modified,
        ),
        FlowChange::New {
            diff, base_broken, ..
        } => {
            let mut summary = String::from("new since base");
            if *base_broken {
                summary.push_str(" (base version did not parse)");
            }
            (render_difference(diff, summary, opts), Marker::New)
        }
        FlowChange::Unchanged { .. } => (normal.clone(), Marker::Same),
        FlowChange::Broken { error, .. } => (
            DiagramView {
                text: ansi::ansi_to_text(&format!("parse error, {error}")),
                summary: "parse error".into(),
                details: Vec::new(),
            },
            Marker::Error,
        ),
        FlowChange::Removed { .. } => {
            let mut view = normal.clone();
            if let Some(Ok(old)) = flows::diagram(previous.unwrap_or(flow)) {
                let empty = Diagram {
                    direction: old.direction,
                    nodes: vec![],
                    edges: vec![],
                    clusters: vec![],
                    node_cluster: vec![],
                };
                view =
                    render_difference(&diff::diff(&old, &empty), "removed since base".into(), opts);
            }
            view.summary = "removed since base".into();
            (view, Marker::Removed)
        }
    }
}

fn render_difference(
    difference: &diff::DiagramDiff,
    summary: String,
    opts: &Options,
) -> DiagramView {
    DiagramView {
        text: ansi::ansi_to_text(&strata::render_diagram(&difference.union, opts)),
        summary,
        details: difference
            .details()
            .into_iter()
            .map(|(_, text)| text)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn flow_source(title: &str, step: &str) -> String {
        format!(
            "# {title}\n\nA customer completes an order.\n\n```mermaid\nflowchart TD\nactor([Customer]) --> pay[{step}]\npay --> done[Receive receipt]\n```\n"
        )
    }

    fn base(reference: &str) -> BrowserBase {
        BrowserBase {
            reference: reference.into(),
            data: None,
            initial: true,
            explicit: false,
        }
    }

    fn git_command(dir: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=strata",
                "-c",
                "user.email=strata@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(arguments)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn load_without_git_and_with_unborn_head() {
        let directory = assert_fs::TempDir::new().unwrap();
        fs::write(
            directory.path().join("pay.md"),
            flow_source("Pay", "Pay order"),
        )
        .unwrap();
        let mut reference = base("HEAD");
        let loaded = load(directory.path(), &mut reference, &Options::default()).unwrap();
        assert!(loaded.base.is_none());
        assert_eq!(loaded.flows[0].marker, Marker::Same);
        git_command(directory.path(), &["init", "-q"]);
        let loaded = load(directory.path(), &mut base("HEAD"), &Options::default()).unwrap();
        assert!(loaded.base.is_none());
        assert!(loaded.status.is_none());
    }

    #[test]
    fn reload_recomputes_git_and_keeps_previous_base_on_failure() {
        let directory = assert_fs::TempDir::new().unwrap();
        let path = directory.path().join("pay.md");
        fs::write(&path, flow_source("Pay", "Pay order")).unwrap();
        git_command(directory.path(), &["init", "-q"]);
        git_command(directory.path(), &["add", "."]);
        git_command(directory.path(), &["commit", "-qm", "base"]);
        let mut reference = base("HEAD");
        let initial = load(directory.path(), &mut reference, &Options::default()).unwrap();
        let sha = initial.base.unwrap().sha;
        fs::write(&path, flow_source("Pay", "Pay securely")).unwrap();
        let modified = load(directory.path(), &mut reference, &Options::default()).unwrap();
        assert_eq!(modified.flows[0].marker, Marker::Modified);
        reference.reference = "missing-ref".into();
        fs::write(&path, flow_source("Updated title", "Pay securely")).unwrap();
        let retained = load(directory.path(), &mut reference, &Options::default()).unwrap();
        assert_eq!(retained.base.unwrap().sha, sha);
        assert_eq!(retained.flows[0].flow.title, "Updated title");
        assert_eq!(
            retained.flows[0].difference.text,
            modified.flows[0].difference.text
        );
        assert!(retained.status.unwrap().starts_with("git reload failed:"));
    }

    #[test]
    fn catalog_stamp_tracks_names_length_and_time() {
        let directory = assert_fs::TempDir::new().unwrap();
        let initial = catalog_stamp(directory.path()).unwrap();
        fs::write(directory.path().join("notes.txt"), "ignored").unwrap();
        let with_notes = catalog_stamp(directory.path()).unwrap();
        assert_ne!(initial, with_notes);
        let initial = with_notes;
        let path = directory.path().join("flow.md");
        fs::write(&path, "first").unwrap();
        let added = catalog_stamp(directory.path()).unwrap();
        assert_ne!(initial, added);
        assert_eq!(added, catalog_stamp(directory.path()).unwrap());
        fs::write(&path, "longer text").unwrap();
        assert_ne!(added, catalog_stamp(directory.path()).unwrap());
        fs::rename(&path, directory.path().join("renamed.md")).unwrap();
        assert_ne!(added, catalog_stamp(directory.path()).unwrap());
        fs::remove_file(directory.path().join("renamed.md")).unwrap();
        assert_eq!(initial, catalog_stamp(directory.path()).unwrap());
    }

    #[test]
    fn loaded_views_cover_changes_errors_and_anchors() {
        let directory = Path::new("tests/fixtures/flows/browse");
        let current = flows::load_dir(directory).unwrap();
        let report = check::check_catalog_with(directory, &mut |_| true).unwrap();
        let loaded = build_loaded(
            directory,
            current.clone(),
            report.clone(),
            "main",
            None,
            &Options::default(),
        );
        let broken = loaded
            .flows
            .iter()
            .find(|entry| entry.flow.name == "broken-flow")
            .unwrap();
        assert_eq!(broken.marker, Marker::Error);
        assert!(
            broken.normal.text.lines[0].spans[0]
                .content
                .starts_with("parse error, line ")
        );
        let pay = current
            .iter()
            .find(|flow| flow.name == "pay-for-an-order")
            .unwrap();
        let previous = strata::flows::parse_flow(
            "pay-for-an-order.md",
            "# Broken base\n\nDescription.\n\n```mermaid\ngraph TD\na -->\n```",
        );
        let changes = BaseChanges {
            base: "1234567890".into(),
            changes: flows::compare(&[previous], &current),
            old: vec![],
            stale: vec![],
        };
        let new = build_entry(directory, pay, &report, Some(&changes), &Options::default());
        assert_eq!(new.marker, Marker::New);
        assert_eq!(
            new.difference.summary,
            "new since base (base version did not parse)"
        );
        assert!(
            new.footer
                .iter()
                .any(|line| line.contains("pay: src/pay.rs"))
        );
        let old = vec![pay.clone()];
        let removed = BaseChanges {
            base: "1234567890".into(),
            changes: flows::compare(&old, &[]),
            old,
            stale: vec![],
        };
        let loaded = build_loaded(
            directory,
            vec![],
            report.clone(),
            "HEAD",
            Some(&removed),
            &Options::default(),
        );
        assert_eq!(loaded.flows[0].marker, Marker::Removed);
        assert_eq!(loaded.flows[0].difference.summary, "removed since base");
        assert!(
            loaded.flows[0]
                .difference
                .text
                .lines
                .iter()
                .any(|line| line.to_string().contains("- Pay"))
        );
        let missing = flows::parse_flow("pay-for-an-order.md", "# Pay\n\nDescription.");
        let missing = build_entry(
            directory,
            &missing,
            &report,
            Some(&removed),
            &Options::default(),
        );
        assert_eq!(
            missing.normal.text.lines[0].spans[0].content,
            "no mermaid block"
        );
        assert!(
            missing
                .difference
                .text
                .lines
                .iter()
                .any(|line| line.to_string().contains("- Pay"))
        );
    }
}
