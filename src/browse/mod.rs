mod ansi;
pub(crate) mod app;
mod layout;
mod ui;
mod watch;

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use anyhow::bail;
use ratatui::crossterm::event::{self, Event};
use strata::flows::{self, Flow, FlowChange};
use strata::{Diagram, Options, anchors, check, diff, git};

use crate::{BaseChanges, DEFAULT_REF, Wake};
use app::{Action, App, BaseInfo, DiagramView, Entry, Loaded, Marker};
use watch::{ReloadWatch, WakeMode};

const KEY_POLL: Duration = Duration::from_millis(250);

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

type Loader = Box<dyn FnMut() -> anyhow::Result<Loaded> + Send>;

struct ReloadWorker {
    loader: Arc<Mutex<Loader>>,
    pending: Option<mpsc::Receiver<anyhow::Result<Loaded>>>,
}

impl ReloadWorker {
    fn new(loader: impl FnMut() -> anyhow::Result<Loaded> + Send + 'static) -> Self {
        Self {
            loader: Arc::new(Mutex::new(Box::new(loader))),
            pending: None,
        }
    }

    fn start(&mut self, app: &mut App) -> bool {
        if self.pending.is_some() {
            return false;
        }
        let loader = Arc::clone(&self.loader);
        let (sender, receiver) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("strata-reload".into())
            .spawn(move || {
                let result = match loader.lock() {
                    Ok(mut loader) => loader(),
                    Err(_) => Err(anyhow::anyhow!("reload loader lock poisoned")),
                };
                let _ = sender.send(result);
            });
        if let Err(error) = spawned {
            app.status = Some(format!("reload failed: {error}"));
            return false;
        }
        self.pending = Some(receiver);
        app.reloading = true;
        true
    }
}

struct BrowserSession {
    dir: PathBuf,
    worker: ReloadWorker,
    watch: ReloadWatch,
    wake: Wake,
    _watcher: Option<notify::RecommendedWatcher>,
}

impl BrowserSession {
    fn new(dir: &Path, reference: Option<&str>, opts: &Options) -> anyhow::Result<(Self, App)> {
        let mut base = BrowserBase {
            reference: reference.unwrap_or(DEFAULT_REF).into(),
            data: None,
            initial: true,
            explicit: reference.is_some(),
        };
        let mut app = App::new(load(dir, &mut base, opts)?, opts.ascii, opts.color);
        let (sender, receiver) = mpsc::channel();
        let (watcher, wake, mode) = match crate::start_watcher(dir, sender) {
            Ok(watcher) => (Some(watcher), Wake::Events(receiver), WakeMode::Events),
            Err(error) => {
                app.status = Some(watcher_message(&error));
                (None, Wake::Poll, WakeMode::Poll)
            }
        };
        let watch = ReloadWatch::new(mode, catalog_stamp(dir)?, Instant::now());
        let directory = dir.to_path_buf();
        let options = opts.clone();
        let worker = ReloadWorker::new(move || load(&directory, &mut base, &options));
        Ok((
            Self {
                dir: dir.to_path_buf(),
                worker,
                watch,
                wake,
                _watcher: watcher,
            },
            app,
        ))
    }

    fn manual_reload(&mut self, app: &mut App) {
        if self.worker.pending.is_some() {
            return;
        }
        match catalog_stamp(&self.dir) {
            Ok(stamp) => self.watch.record_reload(Instant::now(), stamp),
            Err(error) => app.status = Some(format!("catalog: {error:#}")),
        }
        self.worker.start(app);
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
    let (mut session, mut app) = BrowserSession::new(dir, reference, opts)?;
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init()?;
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        match handle_input(&mut app)? {
            Action::Quit => return Ok(ExitCode::SUCCESS),
            Action::Reload => session.manual_reload(&mut app),
            Action::None => {}
        }
        poll_watch(&mut session, &mut app);
        apply_reload(&mut session.worker, &mut app);
    }
}

fn handle_input(app: &mut App) -> anyhow::Result<Action> {
    if event::poll(KEY_POLL)? {
        match event::read()? {
            Event::Key(key) => return Ok(app.handle(key)),
            Event::Resize(cols, rows) => app.resize(cols, rows),
            _ => {}
        }
    }
    Ok(Action::None)
}

fn poll_watch(session: &mut BrowserSession, app: &mut App) {
    let now = Instant::now();
    let mut failure = None;
    if let Wake::Events(receiver) = &session.wake {
        for message in receiver.try_iter() {
            match message {
                Ok(_) => session.watch.on_event(now),
                Err(error) => failure = Some(watcher_message(&error)),
            }
        }
    }
    if let Some(message) = failure {
        app.status = Some(message);
        session.wake = Wake::Poll;
        session.watch.wake = WakeMode::Poll;
    }
    if session.worker.pending.is_some() || !session.watch.is_due(now) {
        return;
    }
    match catalog_stamp(&session.dir) {
        Ok(stamp) => {
            if session.watch.should_reload(now, || stamp) {
                session.worker.start(app);
            }
        }
        Err(error) => app.status = Some(format!("catalog: {error:#}")),
    }
}

fn apply_reload(worker: &mut ReloadWorker, app: &mut App) {
    let Some(receiver) = &worker.pending else {
        return;
    };
    let result = match receiver.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("reload worker disconnected")),
    };
    worker.pending = None;
    app.reloading = false;
    match result {
        Ok(loaded) => app.apply(loaded),
        Err(error) => app.status = Some(format!("reload failed: {error:#}")),
    }
}

fn watcher_message(error: &notify::Error) -> String {
    format!("file events unavailable ({error}); checking every 1s")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StampEntry {
    name: String,
    modified: Option<SystemTime>,
    length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogStamp(Vec<StampEntry>);

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
        files.push(StampEntry {
            name,
            modified,
            length,
        });
    }
    files.sort_by(|left, right| left.name.cmp(&right.name));
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
        Err(error) if base.data.is_some() => status = Some(format!("git reload failed: {error:#}")),
        Err(error) if base.initial && !expected_git_absence(&error) => {
            status = Some(format!("git unavailable: {error:#}"))
        }
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

fn expected_git_absence(error: &anyhow::Error) -> bool {
    match error.downcast_ref::<git::GitError>() {
        Some(git::GitError::Failed { reason, .. }) => {
            reason.contains("not a git repository")
                || reason.contains("Not a valid object name HEAD")
        }
        _ => false,
    }
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
    crate::problem_line(
        problem,
        &location,
        crate::severity_text(problem.code.severity()),
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

    fn run_with_git_ceiling(test_name: &str) -> bool {
        let test_name = format!("browse::tests::{test_name}");
        if std::env::var("STRATA_BROWSE_GIT_TEST").as_deref() == Ok(&test_name) {
            return false;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env("STRATA_BROWSE_GIT_TEST", &test_name)
            .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
        true
    }

    fn git_command(dir: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
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

    fn loaded_with_flows(flows: Vec<Entry>) -> Loaded {
        Loaded {
            flows,
            base: None,
            catalog_problems: 0,
            status: None,
        }
    }

    #[test]
    fn reload_runs_off_the_ui_thread() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let ui_thread = std::thread::current().id();
        let mut worker = ReloadWorker::new(move || {
            started_sender.send(std::thread::current().id()).unwrap();
            release_receiver.recv().unwrap();
            Ok(loaded_with_flows(vec![]))
        });
        let mut app = App::new(loaded_with_flows(vec![]), false, false);
        assert!(worker.start(&mut app));
        let worker_thread = started_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_ne!(worker_thread, ui_thread);
        apply_reload(&mut worker, &mut app);
        assert!(worker.pending.is_some());
        assert!(app.reloading);
        assert!(
            !worker.start(&mut app),
            "a second reload started while one was pending"
        );
        assert_eq!(
            app.handle(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Action::Quit
        );
        assert_eq!(
            app.handle(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::CONTROL)),
            Action::Quit
        );
        release_sender.send(()).unwrap();
        worker
            .pending
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
    }

    #[test]
    fn reload_results_apply_without_losing_data_on_error() {
        let mut app = App::new(loaded_with_flows(vec![]), false, false);
        let mut worker = ReloadWorker::new(|| panic!("completed results need no loader"));
        let mut loaded = loaded_with_flows(vec![]);
        loaded.catalog_problems = 3;
        loaded.base = Some(BaseInfo {
            reference: "HEAD".into(),
            sha: "1234567890".into(),
        });
        let (sender, receiver) = mpsc::sync_channel(1);
        sender.send(Ok(loaded)).unwrap();
        worker.pending = Some(receiver);
        app.reloading = true;
        apply_reload(&mut worker, &mut app);
        assert_eq!(app.catalog_problems, 3);
        assert_eq!(app.base.as_ref().unwrap().sha, "1234567890");
        assert!(!app.reloading);
        assert!(worker.pending.is_none());
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(Err(anyhow::anyhow!("catalog disappeared")))
            .unwrap();
        worker.pending = Some(receiver);
        app.reloading = true;
        apply_reload(&mut worker, &mut app);
        assert_eq!(
            app.status.as_deref(),
            Some("reload failed: catalog disappeared")
        );
        assert_eq!(app.catalog_problems, 3);
        assert_eq!(app.base.as_ref().unwrap().sha, "1234567890");
        assert!(!app.reloading);
        assert!(worker.pending.is_none());
    }

    #[test]
    fn non_git_catalog_reloads_quietly() {
        if run_with_git_ceiling("non_git_catalog_reloads_quietly") {
            return;
        }
        let directory = assert_fs::TempDir::new().unwrap();
        fs::write(
            directory.path().join("pay.md"),
            flow_source("Pay", "Pay order"),
        )
        .unwrap();
        let mut reference = base("HEAD");
        for _ in 0..2 {
            let loaded = load(directory.path(), &mut reference, &Options::default()).unwrap();
            assert!(loaded.status.is_none(), "{:?}", loaded.status);
        }
    }

    #[test]
    fn load_without_git_and_with_unborn_head() {
        if run_with_git_ceiling("load_without_git_and_with_unborn_head") {
            return;
        }
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
        if run_with_git_ceiling("reload_recomputes_git_and_keeps_previous_base_on_failure") {
            return;
        }
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
    fn load_reports_stale_from_real_git() {
        if run_with_git_ceiling("load_reports_stale_from_real_git") {
            return;
        }
        let directory = assert_fs::TempDir::new().unwrap();
        let catalog = directory.path().join("docs/flows");
        fs::create_dir_all(&catalog).unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        let source = flow_source("Pay", "Pay order")
            .replace("pay --> done", "%% @code pay src/pay.rs\npay --> done");
        fs::write(catalog.join("pay.md"), source).unwrap();
        fs::write(directory.path().join("src/pay.rs"), "fn pay() {}\n").unwrap();
        git_command(directory.path(), &["init", "-q"]);
        git_command(directory.path(), &["add", "."]);
        git_command(directory.path(), &["commit", "-qm", "base"]);
        fs::write(
            directory.path().join("src/pay.rs"),
            "fn pay() { changed(); }\n",
        )
        .unwrap();
        let loaded = load(&catalog, &mut base("HEAD"), &Options::default()).unwrap();
        assert_eq!(loaded.flows[0].marker, Marker::Stale);
        assert_eq!(loaded.flows[0].stale, ["pay: src/pay.rs"]);
        assert!(loaded.status.is_none());
    }

    #[test]
    fn unexpected_initial_git_error_is_shown_once() {
        if run_with_git_ceiling("unexpected_initial_git_error_is_shown_once") {
            return;
        }
        let directory = assert_fs::TempDir::new().unwrap();
        fs::write(
            directory.path().join("pay.md"),
            flow_source("Pay", "Pay order"),
        )
        .unwrap();
        git_command(directory.path(), &["init", "-q"]);
        fs::write(directory.path().join(".git/config"), "[broken\n").unwrap();
        let mut reference = base("HEAD");
        let loaded = load(directory.path(), &mut reference, &Options::default()).unwrap();
        assert!(loaded.status.unwrap().starts_with("git unavailable:"));
        let loaded = load(directory.path(), &mut reference, &Options::default()).unwrap();
        assert!(loaded.status.is_none());
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
        let broken_base = BaseChanges {
            base: "1234567890".into(),
            changes: flows::compare(&current, &current),
            old: current.clone(),
            stale: vec![],
        };
        let broken = build_entry(
            directory,
            &broken.flow,
            &report,
            Some(&broken_base),
            &Options::default(),
        );
        assert_eq!(broken.difference.text, broken.normal.text);
        assert_eq!(broken.difference.summary, "parse error");
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
