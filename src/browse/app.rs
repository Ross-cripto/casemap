use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::ListState;
use strata::flows::Flow;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BareMode {
    Browse,
    ReadStdin,
}

pub(crate) fn bare_mode(
    stdin_tty: bool,
    stdout_tty: bool,
    ci: bool,
    term_dumb: bool,
    catalog_exists: bool,
    watch: bool,
    has_file: bool,
) -> BareMode {
    if stdin_tty && stdout_tty && !ci && !term_dumb && catalog_exists && !watch && !has_file {
        BareMode::Browse
    } else {
        BareMode::ReadStdin
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Marker {
    Error,
    Stale,
    Modified,
    New,
    Removed,
    Same,
}

impl Marker {
    pub(super) fn choose(broken: bool, check_errors: bool, stale: bool, change: Self) -> Self {
        if broken || check_errors {
            return Self::Error;
        }
        if stale {
            return Self::Stale;
        }
        change
    }
    pub(super) fn symbol(self, ascii: bool) -> &'static str {
        match (self, ascii) {
            (Self::Error, false) => "✗",
            (Self::Error, true) => "x",
            (Self::Stale, _) => "!",
            (Self::Modified, _) => "~",
            (Self::New, _) => "+",
            (Self::Removed, _) => "-",
            (Self::Same, false) => "·",
            (Self::Same, true) => ".",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct DiagramView {
    pub text: Text<'static>,
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub flow: Flow,
    pub path: String,
    pub marker: Marker,
    pub normal: DiagramView,
    pub difference: DiagramView,
    pub problems: Vec<String>,
    pub footer: Vec<String>,
    pub stale: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct BaseInfo {
    pub reference: String,
    pub sha: String,
}

pub(super) struct Loaded {
    pub flows: Vec<Entry>,
    pub base: Option<BaseInfo>,
    pub catalog_problems: usize,
    pub status: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    List,
    Diagram,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NarrowView {
    List,
    Diagram,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Search {
    Off,
    Typing(String),
    Kept(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    None,
    Quit,
    Reload,
}

pub(super) struct App {
    pub flows: Vec<Entry>,
    pub view: Vec<usize>,
    pub list_state: ListState,
    pub focus: Focus,
    pub narrow_view: NarrowView,
    pub pan: (u16, u16),
    pub viewport: (u16, u16),
    pub search: Search,
    pub diff_mode: bool,
    pub panel: bool,
    pub help: bool,
    pub status: Option<String>,
    pub base: Option<BaseInfo>,
    pub ascii: bool,
    pub color: bool,
    pub area: Rect,
    pub catalog_problems: usize,
}

impl App {
    pub(super) fn new(loaded: Loaded, ascii: bool, color: bool) -> Self {
        let mut app = Self {
            flows: Vec::new(),
            view: Vec::new(),
            list_state: ListState::default(),
            focus: Focus::List,
            narrow_view: NarrowView::List,
            pan: (0, 0),
            viewport: (0, 0),
            search: Search::Off,
            diff_mode: false,
            panel: false,
            help: false,
            status: None,
            base: None,
            ascii,
            color,
            area: Rect::default(),
            catalog_problems: 0,
        };
        app.apply(loaded);
        app
    }
    pub(super) fn handle(&mut self, event: KeyEvent) -> Action {
        if event.kind != KeyEventKind::Press {
            return Action::None;
        }
        self.status = None;
        if event.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(event.code, KeyCode::Char('c' | 'C'))
        {
            return Action::Quit;
        }
        if event.code == KeyCode::Esc {
            return self.escape();
        }
        if self.help {
            return match event.code {
                KeyCode::Char('q') => Action::Quit,
                KeyCode::Char('?') => {
                    self.help = false;
                    Action::None
                }
                _ => Action::None,
            };
        }
        if matches!(self.search, Search::Typing(_)) {
            self.search_key(event);
            return Action::None;
        }
        let action = match event.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('r') => Action::Reload,
            KeyCode::Char('/') => {
                let query = self.query().to_owned();
                self.search = Search::Typing(query);
                Action::None
            }
            KeyCode::Char('?') => {
                self.help = true;
                Action::None
            }
            KeyCode::Char('p') => {
                self.panel = !self.panel;
                Action::None
            }
            KeyCode::Char('d') => {
                if self.base.is_some() {
                    self.diff_mode = !self.diff_mode;
                } else {
                    self.status = Some("diff mode needs git".into());
                }
                Action::None
            }
            KeyCode::Tab => {
                self.toggle_focus();
                Action::None
            }
            KeyCode::Enter
                if self.area.width < super::layout::WIDE_WIDTH && self.focus == Focus::List =>
            {
                self.toggle_focus();
                Action::None
            }
            code => {
                self.navigate(code);
                Action::None
            }
        };
        self.resize(self.area.width, self.area.height);
        action
    }

    fn escape(&mut self) -> Action {
        if self.help {
            self.help = false;
            return Action::None;
        }
        match &self.search {
            Search::Typing(query) => {
                self.search = kept_search(query.clone());
                return Action::None;
            }
            Search::Kept(_) => {
                self.search = Search::Off;
                self.refilter();
                return Action::None;
            }
            Search::Off => {}
        }
        if self.area.width < super::layout::WIDE_WIDTH && self.narrow_view == NarrowView::Diagram {
            self.toggle_focus();
            self.resize(self.area.width, self.area.height);
            return Action::None;
        }
        Action::Quit
    }

    fn search_key(&mut self, event: KeyEvent) {
        match event.code {
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Enter => self.search = kept_search(self.query().to_owned()),
            KeyCode::Backspace => {
                if let Search::Typing(query) = &mut self.search {
                    query.pop();
                }
                self.refilter();
            }
            KeyCode::Char(character) if is_printable(event.modifiers, character) => {
                if let Search::Typing(query) = &mut self.search {
                    query.push(character);
                }
                self.refilter();
            }
            _ => {}
        }
        self.resize(self.area.width, self.area.height);
    }

    fn toggle_focus(&mut self) {
        match self.focus {
            Focus::List => {
                self.focus = Focus::Diagram;
                self.narrow_view = NarrowView::Diagram;
            }
            Focus::Diagram => {
                self.focus = Focus::List;
                self.narrow_view = NarrowView::List;
            }
        }
    }

    fn navigate(&mut self, code: KeyCode) {
        use KeyCode::*;
        if self.focus == Focus::List {
            let areas = super::layout::layout(self.area, self);
            let page = i32::from(areas.list.height.saturating_sub(2).max(1));
            match code {
                Up | Char('k') => self.move_selection(-1),
                Down | Char('j') => self.move_selection(1),
                PageUp => self.move_selection(-page),
                PageDown => self.move_selection(page),
                Home => self.select(0),
                _ => {}
            }
            return;
        }
        const FAST_PAN: u16 = 10;
        match code {
            Up | Char('k') => self.pan.0 = self.pan.0.saturating_sub(1),
            Down | Char('j') => self.pan.0 = self.pan.0.saturating_add(1),
            Left | Char('h') => self.pan.1 = self.pan.1.saturating_sub(1),
            Right | Char('l') => self.pan.1 = self.pan.1.saturating_add(1),
            Char('K') => self.pan.0 = self.pan.0.saturating_sub(FAST_PAN),
            Char('J') => self.pan.0 = self.pan.0.saturating_add(FAST_PAN),
            Char('H') => self.pan.1 = self.pan.1.saturating_sub(FAST_PAN),
            Char('L') => self.pan.1 = self.pan.1.saturating_add(FAST_PAN),
            PageUp => self.pan.0 = self.pan.0.saturating_sub(self.viewport.0),
            PageDown => self.pan.0 = self.pan.0.saturating_add(self.viewport.0),
            Home => self.pan = (0, 0),
            _ => {}
        }
    }

    fn select(&mut self, position: usize) {
        let selection = (!self.view.is_empty()).then(|| position.min(self.view.len() - 1));
        if selection != self.list_state.selected() {
            self.pan = (0, 0);
        }
        self.list_state.select(selection);
    }

    fn move_selection(&mut self, distance: i32) {
        let selected = self.list_state.selected().unwrap_or(0);
        self.select(selected.saturating_add_signed(distance as isize));
    }

    pub(super) fn resize(&mut self, cols: u16, rows: u16) {
        self.area = Rect::new(0, 0, cols, rows);
        let diagram = super::layout::layout(self.area, self).diagram;
        if diagram.width > 0 && diagram.height > 0 {
            self.viewport = (
                diagram.height.saturating_sub(2),
                diagram.width.saturating_sub(2),
            );
        }
        self.clamp_pan();
    }

    fn clamp_pan(&mut self) {
        let (height, width) = self.diagram().map_or((0, 0), |diagram| {
            (
                dimension(diagram.text.height()),
                dimension(diagram.text.width()),
            )
        });
        self.pan.0 = self.pan.0.min(height.saturating_sub(self.viewport.0));
        self.pan.1 = self.pan.1.min(width.saturating_sub(self.viewport.1));
    }

    pub(super) fn apply(&mut self, loaded: Loaded) {
        let path = self.selected().map(|entry| entry.flow.path.clone());
        self.flows = loaded.flows;
        self.base = loaded.base;
        self.status = loaded.status;
        self.catalog_problems = loaded.catalog_problems;
        self.rebuild_view(path);
        self.resize(self.area.width, self.area.height);
    }

    fn query(&self) -> &str {
        match &self.search {
            Search::Off => "",
            Search::Typing(query) | Search::Kept(query) => query,
        }
    }

    fn refilter(&mut self) {
        let path = self.selected().map(|entry| entry.flow.path.clone());
        self.rebuild_view(path);
    }

    fn rebuild_view(&mut self, selected_path: Option<String>) {
        let query = self.query().to_lowercase();
        self.view = self
            .flows
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches_query(&entry.flow, &query))
            .map(|(index, _)| index)
            .collect();
        let same_path = selected_path.as_ref().and_then(|path| {
            self.view
                .iter()
                .position(|index| self.flows[*index].flow.path == *path)
        });
        let position = same_path.unwrap_or_else(|| self.list_state.selected().unwrap_or(0));
        if same_path.is_none() {
            self.pan = (0, 0);
        }
        let pan = self.pan;
        self.select(position);
        if same_path.is_some() {
            self.pan = pan;
        }
    }

    pub(super) fn selected(&self) -> Option<&Entry> {
        self.list_state
            .selected()
            .and_then(|selection| self.view.get(selection))
            .and_then(|index| self.flows.get(*index))
    }
    pub(super) fn diagram(&self) -> Option<&DiagramView> {
        self.selected().map(|entry| {
            if self.diff_mode {
                &entry.difference
            } else {
                &entry.normal
            }
        })
    }
    pub(super) fn panel_lines(&self) -> Vec<String> {
        let Some(entry) = self.selected() else {
            return Vec::new();
        };
        let mut lines = entry.problems.clone();
        lines.push("Code:".into());
        lines.extend(entry.footer.iter().cloned());
        if self.diff_mode {
            lines.extend(entry.difference.details.iter().cloned());
            lines.extend(entry.stale.iter().cloned());
        }
        lines
    }
}

fn matches_query(flow: &Flow, query: &str) -> bool {
    flow.title.to_lowercase().contains(query) || flow.name.to_lowercase().contains(query)
}

fn kept_search(query: String) -> Search {
    if query.is_empty() {
        Search::Off
    } else {
        Search::Kept(query)
    }
}

fn is_printable(modifiers: KeyModifiers, character: char) -> bool {
    !character.is_control()
        && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}

pub(super) fn dimension(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod state_tests {
    use super::*;
    #[test]
    fn bare_mode_table() {
        for inputs in 0u8..128 {
            let stdin_tty = inputs & 1 != 0;
            let stdout_tty = inputs & 2 != 0;
            let ci = inputs & 4 != 0;
            let term_dumb = inputs & 8 != 0;
            let catalog_exists = inputs & 16 != 0;
            let watch = inputs & 32 != 0;
            let has_file = inputs & 64 != 0;
            let expected = if inputs == 19 {
                BareMode::Browse
            } else {
                BareMode::ReadStdin
            };
            assert_eq!(
                bare_mode(
                    stdin_tty,
                    stdout_tty,
                    ci,
                    term_dumb,
                    catalog_exists,
                    watch,
                    has_file
                ),
                expected,
                "inputs: {inputs:07b}"
            );
        }
    }
    fn entry(name: &str) -> Entry {
        let text = (0..100)
            .map(|row| format!("{row:03} {}", "diagram".repeat(30)))
            .collect::<Vec<_>>()
            .join("\n");
        let view = DiagramView {
            text: Text::raw(text),
            summary: "unchanged since base".into(),
            details: Vec::new(),
        };
        Entry {
            flow: strata::flows::parse_flow(
                &format!("{name}.md"),
                &format!("# {name}\n\nDescription.\n\n```mermaid\ngraph TD\na --> b\n```"),
            ),
            path: format!("docs/flows/{name}.md"),
            marker: Marker::Same,
            normal: view.clone(),
            difference: view,
            problems: vec![],
            footer: vec![],
            stale: vec![],
        }
    }
    fn loaded(names: &[&str]) -> Loaded {
        Loaded {
            flows: names.iter().map(|name| entry(name)).collect(),
            base: Some(BaseInfo {
                reference: "HEAD".into(),
                sha: "1234567890".into(),
            }),
            catalog_problems: 0,
            status: None,
        }
    }
    fn app() -> App {
        let mut app = App::new(loaded(&["Alpha", "Beta", "Gamma"]), false, true);
        app.resize(120, 30);
        app
    }
    fn key(app: &mut App, code: KeyCode) -> Action {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE))
    }
    #[test]
    fn keys_move_selection_and_pan() {
        let mut app = app();
        key(&mut app, KeyCode::Down);
        assert_eq!(app.list_state.selected(), Some(1));
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Char('J'));
        key(&mut app, KeyCode::Char('L'));
        assert_eq!(app.pan, (10, 10));
        key(&mut app, KeyCode::Char('k'));
        key(&mut app, KeyCode::Char('h'));
        assert_eq!(app.pan, (9, 9));
        key(&mut app, KeyCode::PageDown);
        assert_eq!(app.pan.0, 9 + app.viewport.0);
        key(&mut app, KeyCode::Home);
        assert_eq!(app.pan, (0, 0));
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::PageDown);
        assert_eq!(app.list_state.selected(), Some(2));
        key(&mut app, KeyCode::Home);
        assert_eq!(app.list_state.selected(), Some(0));
    }
    #[test]
    fn pan_is_clamped_to_the_diagram() {
        let mut app = app();
        key(&mut app, KeyCode::Tab);
        for _ in 0..100 {
            key(&mut app, KeyCode::Char('J'));
            key(&mut app, KeyCode::Char('L'));
        }
        let text = &app.diagram().unwrap().text;
        assert_eq!(
            app.pan,
            (
                (text.height() as u16).saturating_sub(app.viewport.0),
                (text.width() as u16).saturating_sub(app.viewport.1)
            )
        );
        app.resize(300, 200);
        assert_eq!(app.pan, (0, 0));
        app.resize(120, 30);
        key(&mut app, KeyCode::Char('J'));
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Down);
        assert_eq!(app.pan, (0, 0));
    }
    #[test]
    fn search_filters_and_esc_clears() {
        let mut app = app();
        key(&mut app, KeyCode::Char('/'));
        for letter in "bEtA".chars() {
            key(&mut app, KeyCode::Char(letter));
        }
        assert_eq!(app.view, [1]);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.search, Search::Kept("bEtA".into()));
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.view.len(), 3);
        assert_eq!(app.search, Search::Off);
        key(&mut app, KeyCode::Char('/'));
        key(&mut app, KeyCode::Char('z'));
        assert!(app.view.is_empty());
        key(&mut app, KeyCode::Backspace);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.search, Search::Off);
    }
    #[test]
    fn search_captures_command_letters() {
        let mut app = app();
        key(&mut app, KeyCode::Char('/'));
        for letter in "qjkd p?".chars() {
            assert_eq!(key(&mut app, KeyCode::Char(letter)), Action::None);
        }
        assert_eq!(app.search, Search::Typing("qjkd p?".into()));
        assert!(!app.help);
        assert!(!app.panel);
        assert!(!app.diff_mode);
    }
    #[test]
    fn esc_chain_order() {
        let mut app = app();
        app.resize(60, 20);
        key(&mut app, KeyCode::Enter);
        app.search = Search::Typing("Alpha".into());
        app.help = true;
        assert_eq!(key(&mut app, KeyCode::Esc), Action::None);
        assert!(!app.help);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.search, Search::Kept("Alpha".into()));
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.search, Search::Off);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.narrow_view, NarrowView::List);
        assert_eq!(key(&mut app, KeyCode::Esc), Action::Quit);
    }
    #[test]
    fn release_events_are_ignored() {
        let mut app = app();
        let mut event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        event.kind = KeyEventKind::Release;
        assert_eq!(app.handle(event), Action::None);
        event.kind = KeyEventKind::Repeat;
        assert_eq!(app.handle(event), Action::None);
    }
    #[test]
    fn ctrl_c_quits_everywhere() {
        for search in [
            Search::Off,
            Search::Typing("q".into()),
            Search::Kept("a".into()),
        ] {
            for help in [false, true] {
                let mut app = app();
                app.search = search.clone();
                app.help = help;
                assert_eq!(
                    app.handle(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
                    Action::Quit
                );
            }
        }
    }
    #[test]
    fn narrow_enter_and_tab() {
        let mut app = app();
        app.resize(60, 20);
        assert_eq!(app.narrow_view, NarrowView::List);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.narrow_view, NarrowView::Diagram);
        assert_eq!(app.focus, Focus::Diagram);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.narrow_view, NarrowView::List);
        assert_eq!(app.focus, Focus::List);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.narrow_view, NarrowView::Diagram);
    }
    #[test]
    fn diff_toggle_without_git_reports_status() {
        let mut app = app();
        app.base = None;
        key(&mut app, KeyCode::Char('d'));
        assert!(!app.diff_mode);
        assert_eq!(app.status.as_deref(), Some("diff mode needs git"));
        key(&mut app, KeyCode::Down);
        assert!(app.status.is_none());
    }
    #[test]
    fn diff_on_broken_flow_shows_the_error() {
        let mut app = app();
        app.flows[0].difference.text = Text::raw("parse error, line 4: expected a step");
        key(&mut app, KeyCode::Char('d'));
        assert!(app.diff_mode);
        assert_eq!(
            app.diagram().unwrap().text.lines[0].spans[0].content,
            "parse error, line 4: expected a step"
        );
        app.pan = (90, 90);
        key(&mut app, KeyCode::Char('d'));
        assert!(app.pan.0 <= 100 - app.viewport.0);
    }
    #[test]
    fn reload_keeps_selection_by_path() {
        let mut app = app();
        key(&mut app, KeyCode::Down);
        app.pan = (10, 10);
        app.apply(loaded(&["Added", "Alpha", "Beta", "Gamma"]));
        assert_eq!(app.list_state.selected(), Some(2));
        assert_eq!(app.selected().unwrap().flow.name, "Beta");
        assert_eq!(app.pan, (10, 10));
    }
    #[test]
    fn reload_clamps_when_selected_flow_disappears() {
        let mut app = app();
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Down);
        app.apply(loaded(&["Alpha"]));
        assert_eq!(app.list_state.selected(), Some(0));
        assert_eq!(app.pan, (0, 0));
        app.apply(loaded(&[]));
        assert_eq!(app.list_state.selected(), None);
    }
    #[test]
    fn markers_follow_priority() {
        for change in [Marker::Modified, Marker::New, Marker::Removed, Marker::Same] {
            assert_eq!(Marker::choose(true, false, true, change), Marker::Error);
            assert_eq!(Marker::choose(false, true, true, change), Marker::Error);
            assert_eq!(Marker::choose(false, false, true, change), Marker::Stale);
            assert_eq!(Marker::choose(false, false, false, change), change);
        }
    }
}
