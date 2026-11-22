pub(crate) mod app;
mod code_panel;
mod graphic;
mod layout;
mod ui;
mod watch;

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

use anyhow::bail;
use casemap::flow_image::{self, ImageOptions, Theme};
use casemap::flows::{self, Flow, FlowChange};
use casemap::{Diagram, Options, anchors, check, diff, git};
use ratatui::crossterm::event::{self, Event};

use crate::{BaseChanges, DEFAULT_REF, Wake};
use app::{Action, App, BaseInfo, DiagramView, Entry, Graphic, Loaded, Marker, Standing};
use ratatui::text::Text;
use watch::{ReloadWatch, WakeMode};

const KEY_POLL: Duration = Duration::from_millis(250);
const RELOAD_POLL: Duration = Duration::from_millis(50);

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

    fn start(&mut self) -> io::Result<()> {
        if self.pending.is_some() {
            return Ok(());
        }
        let loader = Arc::clone(&self.loader);
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("casemap-reload".into())
            .spawn(move || {
                let result = match loader.lock() {
                    Ok(mut loader) => loader(),
                    Err(_) => Err(anyhow::anyhow!("reload loader lock poisoned")),
                };
                let _ = sender.send(result);
            })?;
        self.pending = Some(receiver);
        Ok(())
    }

    fn is_reloading(&self) -> bool {
        self.pending.is_some()
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
                app.watcher_note = Some(watcher_message(&error));
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
        if self.worker.is_reloading() {
            return;
        }
        match catalog_stamp(&self.dir) {
            Ok(stamp) => self.watch.record_reload(Instant::now(), stamp),
            Err(error) => app.status = Some(format!("catalog: {error:#}")),
        }
        if let Err(error) = self.worker.start() {
            app.status = Some(format!("reload failed: {error}"));
        }
    }
}

pub(crate) fn run(
    dir: &Path,
    reference: Option<&str>,
    opts: &Options,
    theme: &Theme,
) -> anyhow::Result<ExitCode> {
    crate::require_catalog(dir)?;
    if let Some(reference) = reference {
        git::merge_base(dir, reference)?;
    }
    if !io::stdout().is_terminal() {
        bail!("casemap browse needs a terminal");
    }
    let (mut session, mut app) = BrowserSession::new(dir, reference, opts)?;
    let _guard = TerminalGuard;
    let mut terminal = ratatui::try_init()?;
    let mut images = graphic::ImageLayer::new(dir);
    let mut code = code_panel::CodePanel::new(dir);
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    loop {
        app.reloading = session.worker.is_reloading();
        code.refresh(&mut app);
        app.resize(app.area.width, app.area.height);
        let graphics_changed = images.prepare(&mut app, theme);
        if graphics_changed {
            images.clear()?;
            // Full-screen resize resets the back buffer without querying the cursor.
            terminal.resize(app.area)?;
        }
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        if images.paint(graphics_changed)? == graphic::PaintOutcome::NeedsRedraw {
            images.prepare_redraw(&mut app, theme);
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
        }
        let mut editor_status = None;
        match handle_input(&mut app, input_timeout(session.worker.is_reloading()))? {
            Action::Quit => {
                images.clear()?;
                return Ok(ExitCode::SUCCESS);
            }
            Action::Reload => session.manual_reload(&mut app),
            Action::OpenCode => {
                editor_status = handle_open_code(&mut code, &mut terminal, &mut images, &mut app)?;
            }
            Action::None => {}
        }
        poll_watch(&mut session, &mut app);
        apply_reload(&mut session.worker, &mut app);
        let messages: Vec<_> = editor_status
            .into_iter()
            .chain(code.open_failure())
            .collect();
        if !messages.is_empty() {
            app.status = Some(messages.join(" · "));
        }
    }
}

fn handle_open_code(
    code: &mut code_panel::CodePanel,
    terminal: &mut ratatui::DefaultTerminal,
    images: &mut graphic::ImageLayer,
    app: &mut App,
) -> anyhow::Result<Option<String>> {
    let editor = match code.open() {
        Ok(Some(editor)) => editor,
        Ok(None) => return Ok(None),
        Err(error) => return Ok(Some(error.to_string())),
    };
    images.clear()?;
    images.hide();
    terminal.show_cursor()?;
    let (restored_terminal, status) = editor.run()?;
    *terminal = restored_terminal;
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    Ok(status)
}

fn input_timeout(reloading: bool) -> Duration {
    if reloading { RELOAD_POLL } else { KEY_POLL }
}

fn handle_input(app: &mut App, timeout: Duration) -> anyhow::Result<Action> {
    if event::poll(timeout)? {
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
        app.watcher_note = Some(message);
        session.wake = Wake::Poll;
        session.watch.wake = WakeMode::Poll;
    }
    if session.worker.is_reloading() || !session.watch.is_due(now) {
        return;
    }
    match catalog_stamp(&session.dir) {
        Ok(stamp) => {
            if session.watch.should_reload(now, || stamp) {
                if let Err(error) = session.worker.start() {
                    app.status = Some(format!("reload failed: {error}"));
                }
            }
        }
        Err(error) => {
            app.status = Some(format!("catalog: {error:#}"));
            session.watch.skip(now);
        }
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
    let map = casemap::map::load_map(dir)?;
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
        map,
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
    mut current: Vec<Flow>,
    report: check::Report,
    reference: &str,
    base: Option<&BaseChanges>,
    opts: &Options,
    map: Option<casemap::map::Map>,
) -> Loaded {
    let mut entries = Vec::new();
    if let Some(map) = &map {
        let groups = casemap::map::group_flows(map, &current);
        let placements = casemap::map::placements(&groups);
        for group in &groups {
            for flow in &group.flows {
                let mut entry = build_entry(dir, flow, &report, base, opts);
                if let Some(placement) = placements.get(&flow.path) {
                    entry.standing = Standing::Mapped(placement.clone());
                }
                entries.push(entry);
            }
        }
    } else {
        current.sort_by(flows::compare_titles);
        for flow in &current {
            entries.push(build_entry(dir, flow, &report, base, opts));
        }
    }
    if let Some(base) = base {
        for flow in &base.old {
            if !current.iter().any(|current| current.path == flow.path) {
                let mut entry = build_entry(dir, flow, &report, Some(base), opts);
                entry.standing = Standing::Removed;
                entries.push(entry);
            }
        }
    }
    Loaded {
        catalog: crate::catalog_path(dir),
        flows: entries,
        has_map: map.is_some(),
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
    _opts: &Options,
) -> Entry {
    let normal = normal_view(flow);
    let change = base.and_then(|base| {
        base.changes
            .iter()
            .find(|change| changed_flow(change).path == flow.path)
    });
    let stale = base.and_then(|base| base.stale.iter().find(|stale| stale.flow.path == flow.path));
    let historical = base
        .and_then(|base| base.old.iter().find(|old| old.path == flow.path))
        .map(|flow| flow_image::parse_flow(flow).map(|diagram| (flow.clone(), diagram)))
        .transpose();
    let (previous, base_error) = match historical {
        Ok(previous) => (previous, None),
        Err(error) => (None, Some(error)),
    };
    let (mut difference, change_marker) = diff_view(flow, previous.as_ref(), &normal, change);
    if let Some(error) = base_error {
        difference.summary = format!(
            "{} (base flow could not be drawn: {error})",
            difference.summary
        );
    }
    let problems: Vec<_> = report
        .problems
        .iter()
        .filter(|problem| problem.file.as_deref() == Some(&flow.path))
        .collect();
    let check_errors = problems
        .iter()
        .any(|problem| problem.code.severity() == check::Severity::Error);
    let broken = normal.graphic.is_none() && !matches!(change, Some(FlowChange::Removed { .. }));
    Entry {
        flow: flow.clone(),
        standing: Standing::Unmapped,
        path: crate::catalog_file_path(dir, &flow.path),
        marker: Marker::choose(broken, check_errors, stale.is_some(), change_marker),
        normal,
        difference,
        previous,
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

fn normal_view(flow: &Flow) -> DiagramView {
    let (text, graphic) = match flow_image::parse_flow(flow) {
        Ok(diagram) => (Text::default(), Some(graphic_view(flow, diagram, None))),
        Err(flow_image::FlowImageError::Parse(error)) => {
            (Text::raw(format!("parse error, {error}")), None)
        }
        Err(error) => (Text::raw(error.to_string()), None),
    };
    DiagramView {
        text,
        graphic,
        summary: "unchanged since base".into(),
        details: Vec::new(),
    }
}

fn diff_view(
    flow: &Flow,
    previous: Option<&(Flow, Diagram)>,
    normal: &DiagramView,
    change: Option<&FlowChange>,
) -> (DiagramView, Marker) {
    let Some(change) = change else {
        return (normal.clone(), Marker::Same);
    };
    match change {
        FlowChange::Modified { diff, .. } => {
            let Some(graphic) = &normal.graphic else {
                return (normal.clone(), Marker::Error);
            };
            if previous.is_none() {
                let added = diff::all_added(&graphic.diagram);
                return (
                    render_difference(flow, &added, "new since base".into()),
                    Marker::New,
                );
            }
            (
                render_difference(flow, diff, diff.summary()),
                Marker::Modified,
            )
        }
        FlowChange::New { diff, .. } => {
            if normal.graphic.is_none() {
                return (normal.clone(), Marker::Error);
            }
            (
                render_difference(flow, diff, "new since base".into()),
                Marker::New,
            )
        }
        FlowChange::Unchanged { .. } => (normal.clone(), Marker::Same),
        FlowChange::Broken { .. } => (
            DiagramView {
                summary: "parse error".into(),
                ..normal.clone()
            },
            Marker::Error,
        ),
        FlowChange::Removed { .. } => {
            let mut view = normal.clone();
            if let Some((_, old)) = previous {
                let empty = Diagram {
                    direction: old.direction,
                    nodes: vec![],
                    edges: vec![],
                    clusters: vec![],
                    node_cluster: vec![],
                };
                view =
                    render_difference(flow, &diff::diff(old, &empty), "removed since base".into());
            }
            view.summary = "removed since base".into();
            (view, Marker::Removed)
        }
    }
}

fn render_difference(flow: &Flow, difference: &diff::DiagramDiff, summary: String) -> DiagramView {
    DiagramView {
        text: Text::default(),
        graphic: Some(graphic_view(
            flow,
            difference.union.clone(),
            Some(difference.changes.clone()),
        )),
        summary,
        details: difference
            .details()
            .into_iter()
            .map(|(_, text)| text)
            .collect(),
    }
}

fn graphic_view(flow: &Flow, diagram: Diagram, changes: Option<Vec<diff::Change>>) -> Graphic {
    let steps = flow_image::steps(
        flow,
        &diagram,
        ImageOptions {
            changes: changes.as_deref(),
            selected_step: None,
        },
    );
    Graphic {
        diagram,
        changes,
        steps,
    }
}

#[cfg(test)]
fn commerce_loaded(opts: &Options) -> Loaded {
    let directory = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        directory.path().join("README.md"),
        include_str!("../../tests/fixtures/maps/commerce.md"),
    )
    .unwrap();
    let map = casemap::map::load_map(directory.path()).unwrap();
    let current = flows::load_dir(Path::new("tests/fixtures/corpus/commerce")).unwrap();
    build_loaded(
        Path::new("docs/flows"),
        current,
        check::Report {
            flows: 9,
            problems: vec![],
        },
        "HEAD",
        None,
        opts,
        map,
    )
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
        if std::env::var("CASEMAP_BROWSE_GIT_TEST").as_deref() == Ok(&test_name) {
            return false;
        }
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_name, "--nocapture"])
            .env("CASEMAP_BROWSE_GIT_TEST", &test_name)
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
                "user.name=casemap",
                "-c",
                "user.email=casemap@example.com",
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
            catalog: "docs/flows".into(),
            has_map: false,
            flows,
            base: None,
            catalog_problems: 0,
            status: None,
        }
    }

    #[test]
    fn input_timeout_is_short_while_reloading() {
        assert_eq!(input_timeout(true), Duration::from_millis(50));
        assert_eq!(input_timeout(false), Duration::from_millis(250));
    }

    #[test]
    fn worker_is_reloading_until_its_result_is_applied() {
        let (started_sender, started_receiver) = mpsc::sync_channel(1);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let mut worker = ReloadWorker::new(move || {
            started_sender.send(()).unwrap();
            release_receiver.recv().unwrap();
            let mut loaded = loaded_with_flows(vec![]);
            loaded.catalog_problems = 7;
            Ok(loaded)
        });
        let mut app = App::new(loaded_with_flows(vec![]), false, false);
        assert!(!worker.is_reloading());
        worker.start().unwrap();
        started_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(worker.is_reloading());
        apply_reload(&mut worker, &mut app);
        assert!(worker.is_reloading());
        worker.start().unwrap();
        release_sender.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.is_reloading() {
            assert!(Instant::now() < deadline, "reload result was not applied");
            apply_reload(&mut worker, &mut app);
            std::thread::yield_now();
        }
        assert_eq!(app.catalog_problems, 7);
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
        worker.start().unwrap();
        let worker_thread = started_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_ne!(worker_thread, ui_thread);
        apply_reload(&mut worker, &mut app);
        assert!(worker.pending.is_some());
        assert!(worker.is_reloading());
        worker.start().unwrap();
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
        apply_reload(&mut worker, &mut app);
        assert_eq!(app.catalog_problems, 3);
        assert_eq!(app.base.as_ref().unwrap().sha, "1234567890");
        assert!(!worker.is_reloading());
        assert!(worker.pending.is_none());
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(Err(anyhow::anyhow!("catalog disappeared")))
            .unwrap();
        worker.pending = Some(receiver);
        apply_reload(&mut worker, &mut app);
        assert_eq!(
            app.status.as_deref(),
            Some("reload failed: catalog disappeared")
        );
        assert_eq!(app.catalog_problems, 3);
        assert_eq!(app.base.as_ref().unwrap().sha, "1234567890");
        assert!(!worker.is_reloading());
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
            retained.flows[0]
                .difference
                .graphic
                .as_ref()
                .unwrap()
                .changes,
            modified.flows[0]
                .difference
                .graphic
                .as_ref()
                .unwrap()
                .changes
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
    fn list_order_is_by_title_on_initial_load_and_reload() {
        let directory = assert_fs::TempDir::new().unwrap();
        for (name, title) in [
            (
                "add-a-bookmark-from-the-terminal",
                "Add a bookmark from the terminal",
            ),
            ("add-a-bookmark", "Add a bookmark"),
            ("z-first", "add a bookmark"),
        ] {
            fs::write(
                directory.path().join(format!("{name}.md")),
                flow_source(title, "Add bookmark"),
            )
            .unwrap();
        }
        let mut browser_base = base("HEAD");
        for _ in 0..2 {
            let loaded = load(directory.path(), &mut browser_base, &Options::default()).unwrap();
            let names: Vec<_> = loaded
                .flows
                .iter()
                .map(|entry| entry.flow.name.as_str())
                .collect();
            assert_eq!(
                names,
                [
                    "add-a-bookmark",
                    "z-first",
                    "add-a-bookmark-from-the-terminal"
                ]
            );
        }
    }

    #[test]
    fn load_does_not_build_images_for_generic_mermaid() {
        for (path, content) in [
            (
                "example.md",
                "# Flow\n\nDescription.\n\n```mermaid\nflowchart TD\na --> b\n```\n\n```mermaid\nflowchart TD\nc --> d\n```\n",
            ),
            ("example.mmd", "flowchart TD\na --> b"),
        ] {
            let flow = flows::parse_flow(path, content);
            assert!(flows::diagram(&flow).unwrap().is_ok());
            let view = normal_view(&flow);
            assert!(view.graphic.is_none(), "{path}");
            assert!(
                view.text
                    .to_string()
                    .contains("one Mermaid flowchart block")
            );
        }
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
            None,
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
            changed: vec![],
            root: Default::default(),
            prefix: String::new(),
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
        assert!(broken.difference.summary.starts_with("parse error"));
        let pay = current
            .iter()
            .find(|flow| flow.name == "pay-for-an-order")
            .unwrap();
        let previous = casemap::flows::parse_flow(
            "pay-for-an-order.md",
            "# Broken base\n\nDescription.\n\n```mermaid\nflowchart TD\na -->\n```",
        );
        let changes = BaseChanges {
            base: "1234567890".into(),
            changed: vec![],
            root: Default::default(),
            prefix: String::new(),
            changes: flows::compare(std::slice::from_ref(&previous), &current),
            old: vec![previous],
            stale: vec![],
        };
        let new = build_entry(directory, pay, &report, Some(&changes), &Options::default());
        assert_eq!(new.marker, Marker::New);
        assert!(
            new.difference
                .summary
                .contains("base flow could not be drawn:")
        );
        assert!(new.difference.summary.contains("line "));
        assert!(
            new.footer
                .iter()
                .any(|line| line.contains("pay: src/pay.rs"))
        );
        let old = vec![pay.clone()];
        let removed = BaseChanges {
            base: "1234567890".into(),
            changed: vec![],
            root: Default::default(),
            prefix: String::new(),
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
            None,
        );
        assert_eq!(loaded.flows[0].marker, Marker::Removed);
        assert_eq!(loaded.flows[0].difference.summary, "removed since base");
        assert!(
            loaded.flows[0]
                .difference
                .graphic
                .as_ref()
                .unwrap()
                .diagram
                .nodes
                .iter()
                .any(|node| node.mark == casemap::Mark::Removed
                    && node.label.iter().any(|label| label.contains("Pay")))
        );
        let missing = flows::parse_flow("pay-for-an-order.md", "# Pay\n\nDescription.");
        let missing = build_entry(
            directory,
            &missing,
            &report,
            Some(&removed),
            &Options::default(),
        );
        assert!(
            missing
                .normal
                .text
                .to_string()
                .contains("one Mermaid flowchart block")
        );
        assert!(
            missing
                .difference
                .graphic
                .as_ref()
                .unwrap()
                .diagram
                .nodes
                .iter()
                .any(|node| node.mark == casemap::Mark::Removed
                    && node.label.iter().any(|label| label.contains("Pay")))
        );
    }
}
