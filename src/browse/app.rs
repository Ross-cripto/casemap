use casemap::flows::Flow;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::ListState;

pub(super) const FAST_PAN: u16 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Offset {
    pub rows: u16,
    pub columns: u16,
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
    pub catalog: String,
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
    pub catalog: String,
    pub flows: Vec<Entry>,
    pub view: Vec<usize>,
    pub list_state: ListState,
    pub focus: Focus,
    pub pan: Offset,
    pub viewport: Offset,
    pub search: Search,
    pub diff_mode: bool,
    pub panel: bool,
    pub help: bool,
    pub status: Option<String>,
    pub reloading: bool,
    pub base: Option<BaseInfo>,
    pub ascii: bool,
    pub color: bool,
    pub area: Rect,
    pub catalog_problems: usize,
}

impl App {
    pub(super) fn new(loaded: Loaded, ascii: bool, color: bool) -> Self {
        let mut app = Self {
            catalog: String::new(),
            flows: Vec::new(),
            view: Vec::new(),
            list_state: ListState::default(),
            focus: Focus::List,
            pan: Offset::default(),
            viewport: Offset::default(),
            search: Search::Off,
            diff_mode: false,
            panel: false,
            help: false,
            status: None,
            reloading: false,
            base: None,
            ascii,
            color,
            area: Rect::default(),
            catalog_problems: 0,
        };
        app.apply(loaded);
        app
    }
    pub(super) fn handle(&mut self, key: KeyEvent) -> Action {
        if key.kind != KeyEventKind::Press {
            return Action::None;
        }
        self.status = None;
        if key.code == KeyCode::Esc {
            return self.escape();
        }
        if matches!(self.search, Search::Typing(_)) && !self.help && !is_quit_shortcut(key) {
            self.search_key(key);
            return Action::None;
        }
        let action = if let Some(action) = self.global_key(key) {
            action
        } else {
            self.navigate(key.code);
            Action::None
        };
        self.resize(self.area.width, self.area.height);
        action
    }

    fn global_key(&mut self, key: KeyEvent) -> Option<Action> {
        if is_quit_shortcut(key) {
            return Some(Action::Quit);
        }
        if self.help && !matches!(key.code, KeyCode::Char('q' | '?')) {
            return Some(Action::None);
        }
        match key.code {
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('r') => return Some(Action::Reload),
            KeyCode::Char('?') => self.help = !self.help,
            KeyCode::Char('p') => self.panel = !self.panel,
            KeyCode::Char('d') => {
                if self.base.is_some() {
                    self.diff_mode = !self.diff_mode;
                } else {
                    self.status = Some("diff mode needs git".into());
                }
            }
            KeyCode::Char('/') => self.search = Search::Typing(self.query().to_owned()),
            _ => return None,
        }
        Some(Action::None)
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
        if self.area.width < super::layout::WIDE_WIDTH && self.focus == Focus::Diagram {
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
        self.focus = match self.focus {
            Focus::List => Focus::Diagram,
            Focus::Diagram => Focus::List,
        };
    }

    fn opens_narrow_diagram(&self, code: KeyCode) -> bool {
        code == KeyCode::Enter
            && self.area.width < super::layout::WIDE_WIDTH
            && self.focus == Focus::List
    }

    fn navigate(&mut self, code: KeyCode) {
        use KeyCode::*;
        if code == Tab || self.opens_narrow_diagram(code) {
            self.toggle_focus();
            return;
        }
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
        match code {
            Up | Char('k') => self.pan.rows = self.pan.rows.saturating_sub(1),
            Down | Char('j') => self.pan.rows = self.pan.rows.saturating_add(1),
            Left | Char('h') => self.pan.columns = self.pan.columns.saturating_sub(1),
            Right | Char('l') => self.pan.columns = self.pan.columns.saturating_add(1),
            Char('K') => self.pan.rows = self.pan.rows.saturating_sub(FAST_PAN),
            Char('J') => self.pan.rows = self.pan.rows.saturating_add(FAST_PAN),
            Char('H') => self.pan.columns = self.pan.columns.saturating_sub(FAST_PAN),
            Char('L') => self.pan.columns = self.pan.columns.saturating_add(FAST_PAN),
            PageUp => self.pan.rows = self.pan.rows.saturating_sub(self.viewport.rows),
            PageDown => self.pan.rows = self.pan.rows.saturating_add(self.viewport.rows),
            Home => self.pan = Offset::default(),
            _ => {}
        }
    }

    fn select(&mut self, position: usize) {
        let selection = (!self.view.is_empty()).then(|| position.min(self.view.len() - 1));
        if selection != self.list_state.selected() {
            self.pan = Offset::default();
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
            self.viewport = Offset {
                rows: diagram.height.saturating_sub(2),
                columns: diagram.width.saturating_sub(2),
            };
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
        self.pan.rows = self.pan.rows.min(height.saturating_sub(self.viewport.rows));
        self.pan.columns = self
            .pan
            .columns
            .min(width.saturating_sub(self.viewport.columns));
    }

    pub(super) fn apply(&mut self, loaded: Loaded) {
        let path = self.selected().map(|entry| entry.flow.path.clone());
        self.catalog = loaded.catalog;
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
            self.pan = Offset::default();
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
    pub(super) fn panel_line_count(&self) -> usize {
        let Some(entry) = self.selected() else {
            return 0;
        };
        let mut count = entry.problems.len() + 1 + entry.footer.len();
        if self.diff_mode {
            count += entry.difference.details.len() + entry.stale.len();
        }
        count
    }

    pub(super) fn panel_lines(&self) -> Vec<String> {
        let Some(entry) = self.selected() else {
            return Vec::new();
        };
        let mut lines = entry.problems.clone();
        if entry.footer.is_empty() {
            lines.push("no anchors".into());
        } else {
            lines.push("Code:".into());
            lines.extend(entry.footer.iter().cloned());
        }
        if self.diff_mode {
            lines.extend(entry.difference.details.iter().cloned());
            lines.extend(entry.stale.iter().cloned());
        }
        lines
    }
}

fn is_quit_shortcut(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C'))
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
            flow: casemap::flows::parse_flow(
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
    fn loaded_with_flows(names: &[&str]) -> Loaded {
        Loaded {
            catalog: "docs/flows".into(),
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
        let mut app = App::new(loaded_with_flows(&["Alpha", "Beta", "Gamma"]), false, true);
        app.resize(120, 30);
        app
    }
    fn press(app: &mut App, code: KeyCode) -> Action {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE))
    }
    #[test]
    fn quit_and_reload_keys() {
        for focus in [Focus::List, Focus::Diagram] {
            let mut app = app();
            app.focus = focus;
            assert_eq!(press(&mut app, KeyCode::Char('q')), Action::Quit);
            assert_eq!(press(&mut app, KeyCode::Char('r')), Action::Reload);
            let original_panel = app.panel;
            press(&mut app, KeyCode::Char('p'));
            assert_ne!(app.panel, original_panel);
            press(&mut app, KeyCode::Char('p'));
            assert_eq!(app.panel, original_panel);
            press(&mut app, KeyCode::Char('?'));
            assert!(app.help);
            assert_eq!(press(&mut app, KeyCode::Char('q')), Action::Quit);
            press(&mut app, KeyCode::Char('?'));
            assert!(!app.help);
            assert_eq!(app.focus, focus);
        }
        let mut app = app();
        app.focus = Focus::Diagram;
        for (letter, expected) in [
            (
                'J',
                Offset {
                    rows: FAST_PAN,
                    columns: 0,
                },
            ),
            (
                'L',
                Offset {
                    rows: FAST_PAN,
                    columns: FAST_PAN,
                },
            ),
            (
                'K',
                Offset {
                    rows: 0,
                    columns: FAST_PAN,
                },
            ),
            ('H', Offset::default()),
        ] {
            assert_eq!(
                app.handle(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::SHIFT)),
                Action::None
            );
            assert_eq!(app.pan, expected, "SHIFT+{letter}");
        }
        press(&mut app, KeyCode::Char('/'));
        for letter in "QJKDPRHL".chars() {
            assert_eq!(
                app.handle(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::SHIFT)),
                Action::None
            );
        }
        assert_eq!(app.search, Search::Typing("QJKDPRHL".into()));
    }

    #[test]
    fn resize_reclamps_both_axes() {
        let mut app = app();
        app.pan = Offset {
            rows: u16::MAX,
            columns: u16::MAX,
        };
        app.resize(120, 30);
        assert_eq!(
            app.pan,
            Offset {
                rows: 76,
                columns: 128
            }
        );
        app.resize(118, 28);
        assert_eq!(
            app.pan,
            Offset {
                rows: 76,
                columns: 128
            }
        );
        app.resize(122, 32);
        assert_eq!(
            app.pan,
            Offset {
                rows: 74,
                columns: 126
            }
        );
    }

    #[test]
    fn search_matches_names_and_keeps_selection_by_path() {
        let mut loaded = loaded_with_flows(&["Other", "pay-for-an-order", "pay-for-a-friend"]);
        loaded.flows[1].flow.title = "Pay for an order".into();
        loaded.flows[2].flow.title = "Pay for a friend".into();
        let mut app = App::new(loaded, false, false);
        app.resize(120, 30);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('/'));
        for letter in "pay-for".chars() {
            press(&mut app, KeyCode::Char(letter));
        }
        assert_eq!(app.view, [1, 2]);
        assert_eq!(app.list_state.selected(), Some(0));
        assert_eq!(app.selected().unwrap().flow.path, "pay-for-an-order.md");
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected().unwrap().flow.path, "pay-for-a-friend.md");
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected().unwrap().flow.path, "pay-for-an-order.md");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.view, [0, 1, 2]);
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn empty_search_escape_and_empty_view_navigation() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.search, Search::Typing(String::new()));
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        assert_eq!(app.search, Search::Off);
        app.apply(loaded_with_flows(&[]));
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::Char('j'),
            KeyCode::Char('k'),
        ] {
            assert_eq!(press(&mut app, code), Action::None);
            assert!(app.selected().is_none());
            assert_eq!(app.pan, Offset::default());
        }
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Down);
        assert!(app.selected().is_none());
    }

    #[test]
    fn keys_move_selection_and_pan() {
        let mut app = app();
        press(&mut app, KeyCode::Down);
        assert_eq!(app.list_state.selected(), Some(1));
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('J'));
        press(&mut app, KeyCode::Char('L'));
        assert_eq!(
            app.pan,
            Offset {
                rows: 10,
                columns: 10
            }
        );
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(
            app.pan,
            Offset {
                rows: 9,
                columns: 9
            }
        );
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.pan.rows, 9 + app.viewport.rows);
        press(&mut app, KeyCode::Home);
        assert_eq!(
            app.pan,
            Offset {
                rows: 0,
                columns: 0
            }
        );
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.list_state.selected(), Some(2));
        press(&mut app, KeyCode::Home);
        assert_eq!(app.list_state.selected(), Some(0));
    }
    #[test]
    fn pan_is_clamped_to_the_diagram() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        for _ in 0..100 {
            press(&mut app, KeyCode::Char('J'));
            press(&mut app, KeyCode::Char('L'));
        }
        let text = &app.diagram().unwrap().text;
        assert_eq!(
            app.pan,
            Offset {
                rows: (text.height() as u16).saturating_sub(app.viewport.rows),
                columns: (text.width() as u16).saturating_sub(app.viewport.columns),
            }
        );
        app.resize(300, 200);
        assert_eq!(
            app.pan,
            Offset {
                rows: 0,
                columns: 0
            }
        );
        app.resize(120, 30);
        press(&mut app, KeyCode::Char('J'));
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Down);
        assert_eq!(
            app.pan,
            Offset {
                rows: 0,
                columns: 0
            }
        );
    }
    #[test]
    fn search_filters_and_esc_clears() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        for letter in "bEtA".chars() {
            press(&mut app, KeyCode::Char(letter));
        }
        assert_eq!(app.view, [1]);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.search, Search::Kept("bEtA".into()));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.view.len(), 3);
        assert_eq!(app.search, Search::Off);
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('z'));
        assert!(app.view.is_empty());
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.search, Search::Off);
    }
    #[test]
    fn search_captures_command_letters() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        for letter in "qjkd p?".chars() {
            assert_eq!(press(&mut app, KeyCode::Char(letter)), Action::None);
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
        press(&mut app, KeyCode::Enter);
        app.search = Search::Typing("Alpha".into());
        app.help = true;
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        assert!(!app.help);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.search, Search::Kept("Alpha".into()));
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.search, Search::Off);
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, Focus::List);
        assert_eq!(press(&mut app, KeyCode::Esc), Action::Quit);
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
                for letter in ['c', 'C'] {
                    assert_eq!(
                        app.handle(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CONTROL)),
                        Action::Quit
                    );
                }
            }
        }
    }
    #[test]
    fn narrow_enter_and_tab() {
        let mut app = app();
        app.resize(60, 20);
        assert_eq!(app.focus, Focus::List);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.focus, Focus::Diagram);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.focus, Focus::List);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.focus, Focus::Diagram);
    }
    #[test]
    fn diff_toggle_without_git_reports_status() {
        let mut app = app();
        app.base = None;
        press(&mut app, KeyCode::Char('d'));
        assert!(!app.diff_mode);
        assert_eq!(app.status.as_deref(), Some("diff mode needs git"));
        press(&mut app, KeyCode::Down);
        assert!(app.status.is_none());
    }
    #[test]
    fn diff_on_broken_flow_shows_the_error() {
        let mut app = app();
        app.flows[0].difference.text = Text::raw("parse error, line 4: expected a step");
        press(&mut app, KeyCode::Char('d'));
        assert!(app.diff_mode);
        assert_eq!(
            app.diagram().unwrap().text.lines[0].spans[0].content,
            "parse error, line 4: expected a step"
        );
        app.pan = Offset {
            rows: 90,
            columns: 90,
        };
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(
            app.pan,
            Offset {
                rows: 76,
                columns: 90
            }
        );
    }
    #[test]
    fn reload_keeps_selection_by_path() {
        let mut app = app();
        press(&mut app, KeyCode::Down);
        app.pan = Offset {
            rows: 10,
            columns: 10,
        };
        app.apply(loaded_with_flows(&["Added", "Alpha", "Beta", "Gamma"]));
        assert_eq!(app.list_state.selected(), Some(2));
        assert_eq!(app.selected().unwrap().flow.name, "Beta");
        assert_eq!(
            app.pan,
            Offset {
                rows: 10,
                columns: 10
            }
        );
    }
    #[test]
    fn reload_clamps_when_selected_flow_disappears() {
        let mut app = app();
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        app.apply(loaded_with_flows(&["Alpha"]));
        assert_eq!(app.list_state.selected(), Some(0));
        assert_eq!(
            app.pan,
            Offset {
                rows: 0,
                columns: 0
            }
        );
        app.apply(loaded_with_flows(&[]));
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
