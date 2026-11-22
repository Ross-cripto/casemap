use casemap::flow_image::StepPosition;
use casemap::flows::Flow;
use casemap::map::{Placement, UNMAPPED_HEADING};
use casemap::{Diagram, diff::Change};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::ListState;

pub(super) const FAST_PAN: u16 = 10;
pub(super) const FIT_ZOOM: u16 = 100;
const MAX_ZOOM: u16 = 300;
const ZOOM_STEP: u16 = 25;

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
    pub graphic: Option<Graphic>,
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct Graphic {
    pub diagram: Diagram,
    pub changes: Option<Vec<Change>>,
    pub steps: Vec<StepPosition>,
}

/// Where an entry sits in the grouped list.
#[derive(Clone, Debug)]
pub(super) enum Standing {
    Mapped(Placement),
    Unmapped,
    Removed,
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub flow: Flow,
    pub standing: Standing,
    pub path: String,
    pub marker: Marker,
    pub normal: DiagramView,
    pub difference: DiagramView,
    pub previous: Option<(Flow, Diagram)>,
    pub problems: Vec<String>,
    pub footer: Vec<String>,
    pub stale: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct BaseInfo {
    pub reference: String,
    pub sha: String,
}

impl BaseInfo {
    pub(super) fn label(&self) -> String {
        crate::base_label(&self.reference, &self.sha)
    }
}

pub(super) struct Loaded {
    pub has_map: bool,
    pub catalog: String,
    /// Display order. With a map: mapped entries in activity order, then unmapped by title,
    /// then removed. `rebuild_view` starts a heading whenever the group changes, so this
    /// order is what keeps one heading per group.
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
pub(super) enum ViewMode {
    Normal,
    Diff,
    SideBySide,
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
    OpenCode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ListRow {
    Heading(String),
    Flow(usize),
}

const REMOVED_HEADING: &str = "Removed";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupKey {
    Activity(usize),
    Unmapped,
    Removed,
}

fn entry_group(entry: &Entry) -> (GroupKey, &str) {
    match &entry.standing {
        Standing::Mapped(placement) => (
            GroupKey::Activity(placement.activity_index),
            &placement.activity,
        ),
        Standing::Unmapped => (GroupKey::Unmapped, UNMAPPED_HEADING),
        Standing::Removed => (GroupKey::Removed, REMOVED_HEADING),
    }
}

pub(super) struct App {
    pub catalog: String,
    pub flows: Vec<Entry>,
    pub view: Vec<usize>,
    pub rows: Vec<ListRow>,
    has_map: bool,
    pub list_state: ListState,
    pub focus: Focus,
    pub pan: Offset,
    pub viewport: Offset,
    pub search: Search,
    pub view_mode: ViewMode,
    selected_node: Option<usize>,
    zoom: u16,
    pub reveal_step: bool,
    pub image_notice: Option<String>,
    pub source_preview: Vec<String>,
    pub image_extent: Option<Offset>,
    pub revision: u64,
    pub panel: bool,
    pub help: bool,
    pub status: Option<String>,
    pub watcher_note: Option<String>,
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
            rows: Vec::new(),
            has_map: false,
            list_state: ListState::default(),
            focus: Focus::List,
            pan: Offset::default(),
            viewport: Offset::default(),
            search: Search::Off,
            view_mode: ViewMode::Normal,
            selected_node: None,
            zoom: FIT_ZOOM,
            reveal_step: false,
            image_notice: None,
            source_preview: Vec::new(),
            image_extent: None,
            revision: 0,
            panel: false,
            help: false,
            status: None,
            watcher_note: None,
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
    pub(super) fn status_text(&self) -> Option<&str> {
        self.status.as_deref().or(self.watcher_note.as_deref())
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
        } else if self.focus == Focus::Diagram && key.code == KeyCode::Enter {
            Action::OpenCode
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
                let mode = match self.view_mode {
                    ViewMode::Normal => ViewMode::Diff,
                    ViewMode::Diff | ViewMode::SideBySide => ViewMode::Normal,
                };
                self.set_view_mode(mode);
            }
            KeyCode::Char('v') => {
                let mode = match self.view_mode {
                    ViewMode::Normal | ViewMode::Diff => ViewMode::SideBySide,
                    ViewMode::SideBySide => ViewMode::Diff,
                };
                self.set_view_mode(mode);
            }
            KeyCode::Char('/') => self.search = Search::Typing(self.query().to_owned()),
            _ => return None,
        }
        Some(Action::None)
    }

    fn set_view_mode(&mut self, mode: ViewMode) {
        if self.base.is_none() {
            self.status = Some("diff mode needs git".into());
            return;
        }
        let node_id = self.selected_node_id().map(str::to_owned);
        self.view_mode = mode;
        self.select_node_by_id(node_id.as_deref());
        if self.selected_node.is_none() {
            self.selected_node = self.steps().first().map(|step| step.node_index);
        }
        self.reveal_step = true;
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
        if self.focus == Focus::Diagram {
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
            Focus::List if self.selected().is_some() => {
                self.panel = true;
                self.reveal_step = true;
                Focus::Diagram
            }
            Focus::List => Focus::List,
            Focus::Diagram => Focus::List,
        };
    }

    fn navigate(&mut self, code: KeyCode) {
        use KeyCode::*;
        if code == Tab || (code == Enter && self.focus == Focus::List) {
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
            Up | Left => self.move_step(-1),
            Down | Right => self.move_step(1),
            Char('k') => self.pan.rows = self.pan.rows.saturating_sub(1),
            Char('j') => self.pan.rows = self.pan.rows.saturating_add(1),
            Char('h') => self.pan.columns = self.pan.columns.saturating_sub(1),
            Char('l') => self.pan.columns = self.pan.columns.saturating_add(1),
            Char('K') => self.pan.rows = self.pan.rows.saturating_sub(FAST_PAN),
            Char('J') => self.pan.rows = self.pan.rows.saturating_add(FAST_PAN),
            Char('H') => self.pan.columns = self.pan.columns.saturating_sub(FAST_PAN),
            Char('L') => self.pan.columns = self.pan.columns.saturating_add(FAST_PAN),
            PageUp => self.pan.rows = self.pan.rows.saturating_sub(self.viewport.rows),
            PageDown => self.pan.rows = self.pan.rows.saturating_add(self.viewport.rows),
            Home => self.pan = Offset::default(),
            Char('+' | '=') => self.zoom_in(),
            Char('-') => self.zoom_out(),
            _ => {}
        }
    }

    pub(super) fn zoom(&self) -> u16 {
        self.zoom
    }

    fn zoom_in(&mut self) {
        self.zoom = self.zoom.saturating_add(ZOOM_STEP).min(MAX_ZOOM);
        self.reveal_step = true;
    }

    fn zoom_out(&mut self) {
        self.zoom = self.zoom.saturating_sub(ZOOM_STEP).max(FIT_ZOOM);
        self.reveal_step = true;
    }

    fn reset_zoom(&mut self) {
        self.zoom = FIT_ZOOM;
        self.pan = Offset::default();
    }

    fn select(&mut self, position: usize) {
        let previous_position = self.selected_position();
        self.select_row(position);
        if self.selected_position() != previous_position {
            self.reset_flow_view();
        }
    }

    fn reset_flow_view(&mut self) {
        self.selected_node = self.steps().first().map(|step| step.node_index);
        self.reset_zoom();
        self.image_extent = None;
        self.reveal_step = true;
    }

    fn select_row(&mut self, position: usize) {
        let selection = (!self.view.is_empty()).then(|| position.min(self.view.len() - 1));
        let row = selection.and_then(|position| self.row_of(position));
        self.list_state.select(row);
        if let Some(row) = row
            && row > 0
            && matches!(self.rows[row - 1], ListRow::Heading(_))
            && self.list_state.offset() > row - 1
        {
            *self.list_state.offset_mut() = row - 1;
        }
    }

    fn row_of(&self, view_position: usize) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| *row == ListRow::Flow(view_position))
    }

    fn selected_position(&self) -> Option<usize> {
        match self.rows.get(self.list_state.selected()?)? {
            ListRow::Flow(position) => Some(*position),
            ListRow::Heading(_) => None,
        }
    }

    fn move_selection(&mut self, distance: i32) {
        let selected = self.selected_position().unwrap_or(0);
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
        let extent = self.image_extent.unwrap_or_else(|| {
            self.diagram().map_or(Offset::default(), |diagram| Offset {
                rows: dimension(diagram.text.height()),
                columns: dimension(diagram.text.width()),
            })
        });
        self.pan.rows = self
            .pan
            .rows
            .min(extent.rows.saturating_sub(self.viewport.rows));
        self.pan.columns = self
            .pan
            .columns
            .min(extent.columns.saturating_sub(self.viewport.columns));
    }

    pub(super) fn set_image_extent(&mut self, extent: Offset) {
        self.image_extent = Some(extent);
        self.clamp_pan();
    }

    fn move_step(&mut self, distance: isize) {
        let steps = self.steps();
        let position = self.selected_step_position().map_or(0, |position| {
            position
                .saturating_add_signed(distance)
                .min(steps.len().saturating_sub(1))
        });
        self.selected_node = steps.get(position).map(|step| step.node_index);
        self.reveal_step = true;
    }

    pub(super) fn selected_step(&self) -> Option<&StepPosition> {
        self.steps().get(self.selected_step_position()?)
    }

    fn steps(&self) -> &[StepPosition] {
        self.diagram()
            .and_then(|view| view.graphic.as_ref())
            .map_or(&[], |graphic| &graphic.steps)
    }

    fn selected_step_position(&self) -> Option<usize> {
        let selected_node = self.selected_node?;
        self.steps()
            .iter()
            .position(|step| step.node_index == selected_node)
    }

    fn selected_node_id(&self) -> Option<&str> {
        let graphic = self.diagram()?.graphic.as_ref()?;
        let node_index = self.selected_step()?.node_index;
        Some(&graphic.diagram.nodes.get(node_index)?.id)
    }

    fn select_node_by_id(&mut self, node_id: Option<&str>) {
        self.selected_node = node_id.and_then(|node_id| {
            let graphic = self.diagram()?.graphic.as_ref()?;
            graphic.steps.iter().find_map(|step| {
                let node = graphic.diagram.nodes.get(step.node_index)?;
                (node.id == node_id).then_some(step.node_index)
            })
        });
    }

    pub(super) fn apply(&mut self, loaded: Loaded) {
        self.revision = self.revision.wrapping_add(1);
        let path = self.selected().map(|entry| entry.flow.path.clone());
        let node_id = self.selected_node_id().map(str::to_owned);
        self.has_map = loaded.has_map;
        self.catalog = loaded.catalog;
        self.flows = loaded.flows;
        self.base = loaded.base;
        if self.base.is_none() {
            self.view_mode = ViewMode::Normal;
        }
        self.status = loaded.status;
        self.catalog_problems = loaded.catalog_problems;
        if self.rebuild_view(path.as_deref()) {
            self.select_node_by_id(node_id.as_deref());
        }
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
        self.rebuild_view(path.as_deref());
    }

    fn rebuild_view(&mut self, selected_path: Option<&str>) -> bool {
        let previous_position = self.selected_position().unwrap_or(0);
        let query = self.query().to_lowercase();
        self.view = self
            .flows
            .iter()
            .enumerate()
            .filter(|(_, entry)| matches_query(&entry.flow, &query))
            .map(|(index, _)| index)
            .collect();
        let same_path = selected_path.and_then(|path| {
            self.view
                .iter()
                .position(|index| self.flows[*index].flow.path == path)
        });
        self.rows.clear();
        let mut previous_group = None;
        for (position, index) in self.view.iter().enumerate() {
            if self.has_map {
                let (key, heading) = entry_group(&self.flows[*index]);
                if previous_group != Some(key) {
                    self.rows.push(ListRow::Heading(heading.into()));
                    previous_group = Some(key);
                }
            }
            self.rows.push(ListRow::Flow(position));
        }
        let position = same_path.unwrap_or(previous_position);
        self.select_row(position);
        if same_path.is_none() {
            self.reset_flow_view();
        }
        same_path.is_some()
    }

    pub(super) fn selected(&self) -> Option<&Entry> {
        self.selected_position()
            .and_then(|selection| self.view.get(selection))
            .and_then(|index| self.flows.get(*index))
    }
    pub(super) fn diagram(&self) -> Option<&DiagramView> {
        self.selected().map(|entry| match self.view_mode {
            ViewMode::Normal => &entry.normal,
            ViewMode::Diff | ViewMode::SideBySide => &entry.difference,
        })
    }
    pub(super) fn panel_line_count(&self) -> usize {
        self.panel_lines().len()
    }

    pub(super) fn panel_lines(&self) -> Vec<String> {
        let Some(entry) = self.selected() else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        if let Some(step) = self.selected_step()
            && let Some(graphic) = self.diagram().and_then(|view| view.graphic.as_ref())
        {
            let node = &graphic.diagram.nodes[step.node_index];
            lines.push(node.label.join(" "));
            if !step.returns_from.is_empty() {
                let origins = step
                    .returns_from
                    .iter()
                    .map(|&index| graphic.diagram.nodes[index].label.join(" "))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("Back from {origins}"));
            }
            lines.extend(self.source_preview.iter().cloned());
        } else if entry.footer.is_empty() {
            lines.push("no anchors".into());
        } else {
            lines.push("Code:".into());
            lines.extend(entry.footer.iter().cloned());
        }
        lines.extend(entry.problems.iter().cloned());
        if self.view_mode != ViewMode::Normal {
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
            graphic: None,
            summary: "unchanged since base".into(),
            details: Vec::new(),
        };
        Entry {
            flow: casemap::flows::parse_flow(
                &format!("{name}.md"),
                &format!("# {name}\n\nDescription.\n\n```mermaid\ngraph TD\na --> b\n```"),
            ),
            standing: Standing::Unmapped,
            path: format!("docs/flows/{name}.md"),
            marker: Marker::Same,
            normal: view.clone(),
            difference: view,
            previous: None,
            problems: vec![],
            footer: vec![],
            stale: vec![],
        }
    }
    fn loaded_with_flows(names: &[&str]) -> Loaded {
        Loaded {
            catalog: "docs/flows".into(),
            has_map: false,
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

    fn grouped_app() -> App {
        let flows = vec![
            entry("alpha").flow,
            entry("beta").flow,
            entry("gamma").flow,
            entry("unmapped").flow,
        ];
        let directory = assert_fs::TempDir::new().unwrap();
        std::fs::write(
            directory.path().join("README.md"),
            "## First\n- [Alpha](alpha.md)\n- [Beta](beta.md)\n## Second\n- [Gamma](gamma.md)\n",
        )
        .unwrap();
        let map = casemap::map::load_map(directory.path()).unwrap();
        let loaded = crate::browse::build_loaded(
            directory.path(),
            flows,
            casemap::check::Report {
                flows: 4,
                problems: vec![],
            },
            "HEAD",
            None,
            &casemap::Options::default(),
            map,
        );
        let mut app = App::new(loaded, false, false);
        app.resize(120, 30);
        app
    }

    #[test]
    fn grouped_rows_follow_the_map() {
        let app = grouped_app();
        assert_eq!(
            app.rows,
            [
                ListRow::Heading("First".into()),
                ListRow::Flow(0),
                ListRow::Flow(1),
                ListRow::Heading("Second".into()),
                ListRow::Flow(2),
                ListRow::Heading(UNMAPPED_HEADING.into()),
                ListRow::Flow(3)
            ]
        );
    }

    #[test]
    fn down_and_up_cross_a_heading() {
        let mut app = grouped_app();
        app.select(1);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected().unwrap().flow.name, "gamma");
        assert_eq!(app.list_state.selected(), Some(4));
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected().unwrap().flow.name, "beta");
    }

    #[test]
    fn up_at_the_first_flow_stays() {
        let mut app = grouped_app();
        app.select(0);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected().unwrap().flow.name, "alpha");
    }

    #[test]
    fn home_scrolls_the_first_heading_into_view() {
        const OFFSET_PAST_FIRST_HEADING: usize = 4;
        let mut app = grouped_app();
        *app.list_state.offset_mut() = OFFSET_PAST_FIRST_HEADING;
        press(&mut app, KeyCode::Home);
        assert_eq!(app.selected().unwrap().flow.name, "alpha");
        assert_eq!(app.list_state.offset(), 0);
    }

    fn assert_selection_is_a_flow(app: &App) {
        assert!(matches!(
            app.rows[app.list_state.selected().unwrap()],
            ListRow::Flow(_)
        ));
    }

    #[test]
    fn paging_never_selects_a_heading() {
        let mut app = grouped_app();
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected().unwrap().flow.name, "unmapped");
        assert_selection_is_a_flow(&app);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.selected().unwrap().flow.name, "alpha");
        app.resize(120, 6);
        for key in [KeyCode::PageDown, KeyCode::PageUp] {
            for _ in 0..8 {
                press(&mut app, key);
                assert_selection_is_a_flow(&app);
            }
        }
        press(&mut app, KeyCode::Home);
        assert_eq!(app.list_state.offset(), 0);
    }

    #[test]
    fn same_name_activities_and_removed_flows_keep_their_headings() {
        let directory = assert_fs::TempDir::new().unwrap();
        let current = vec![entry("alpha").flow, entry("beta").flow];
        let old = vec![entry("removed").flow];
        let base = crate::BaseChanges {
            base: "1234567890".into(),
            changes: casemap::flows::compare(&old, &current),
            old,
            changed: vec![],
            stale: vec![],
            root: directory.path().into(),
            prefix: String::new(),
        };
        let map = casemap::map::parse_map(
            "README.md",
            concat!(
                "## Pay\n",
                "- [Alpha](alpha.md)\n",
                "## Pay\n",
                "- [Beta](beta.md)\n"
            ),
        );
        let loaded = crate::browse::build_loaded(
            directory.path(),
            current,
            casemap::check::Report {
                flows: 2,
                problems: vec![],
            },
            "HEAD",
            Some(&base),
            &casemap::Options::default(),
            map,
        );
        let app = App::new(loaded, false, false);
        let headings: Vec<_> = app
            .rows
            .iter()
            .filter_map(|row| match row {
                ListRow::Heading(name) => Some(name.as_str()),
                ListRow::Flow(_) => None,
            })
            .collect();
        assert_eq!(headings, ["Pay", "Pay", "Removed"]);
    }

    #[test]
    fn search_keeps_headings_of_matching_groups() {
        let mut app = grouped_app();
        press(&mut app, KeyCode::Char('/'));
        for character in "gamma".chars() {
            press(&mut app, KeyCode::Char(character));
        }
        assert_eq!(
            app.rows,
            [ListRow::Heading("Second".into()), ListRow::Flow(0)]
        );
        assert_eq!(app.selected().unwrap().flow.name, "gamma");
        assert_eq!(app.list_state.selected(), Some(1));
        press(&mut app, KeyCode::Char('x'));
        assert!(app.rows.is_empty());
        assert!(app.selected().is_none());
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.selected().unwrap().flow.name, "gamma");
    }

    #[test]
    fn grouped_reload_preserves_selection_after_heading_changes() {
        let mut app = grouped_app();
        app.select(2);
        let mut loaded = loaded_with_flows(&["gamma", "alpha"]);
        loaded.has_map = true;
        loaded.flows[0].standing = Standing::Mapped(Placement {
            activity: "New group".into(),
            activity_index: 0,
            position: 1,
            count: 1,
        });
        app.apply(loaded);
        assert_eq!(app.selected().unwrap().flow.name, "gamma");
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn watcher_note_survives_reloads_and_keys() {
        let mut app = app();
        let note = "file events unavailable; checking every 1s";
        app.watcher_note = Some(note.into());
        app.status = Some("catalog error".into());
        assert_eq!(app.status_text(), Some("catalog error"));
        app.apply(loaded_with_flows(&[]));
        assert_eq!(app.status_text(), Some(note));
        app.status = Some("reload error".into());
        app.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(app.status.is_none());
        assert_eq!(app.status_text(), Some(note));
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
        app.focus = Focus::Diagram;
        app.pan = Offset {
            rows: u16::MAX,
            columns: u16::MAX,
        };
        app.resize(120, 30);
        assert_eq!(
            app.pan,
            Offset {
                rows: 76,
                columns: 96
            }
        );
        app.resize(118, 28);
        assert_eq!(
            app.pan,
            Offset {
                rows: 76,
                columns: 96
            }
        );
        app.resize(122, 32);
        assert_eq!(
            app.pan,
            Offset {
                rows: 74,
                columns: 94
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
        assert_eq!(app.view_mode, ViewMode::Normal);
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
    fn map_opens_the_selected_flow_and_esc_returns_to_it_on_a_wide_terminal() {
        let mut app = app();
        app.resize(120, 40);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected().unwrap().flow.name, "Beta");
        assert!(
            super::super::layout::layout(app.area, &app)
                .diagram
                .is_empty()
        );
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.focus, Focus::Diagram);
        assert_eq!(press(&mut app, KeyCode::Esc), Action::None);
        assert_eq!(app.focus, Focus::List);
        assert_eq!(app.selected().unwrap().flow.name, "Beta");
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
        assert_eq!(app.view_mode, ViewMode::Normal);
        assert_eq!(app.status.as_deref(), Some("diff mode needs git"));
        press(&mut app, KeyCode::Down);
        assert!(app.status.is_none());
    }
    #[test]
    fn diff_on_broken_flow_shows_the_error() {
        let mut app = app();
        app.focus = Focus::Diagram;
        app.flows[0].difference.text = Text::raw("parse error, line 4: expected a step");
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.view_mode, ViewMode::Diff);
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
    fn zoom_is_clamped() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('-'));
        assert_eq!(app.zoom(), FIT_ZOOM);
        for _ in 0..20 {
            press(&mut app, KeyCode::Char('+'));
        }
        assert_eq!(app.zoom(), MAX_ZOOM);
    }

    #[test]
    fn side_by_side_needs_a_base() {
        let mut app = app();
        press(&mut app, KeyCode::Char('v'));
        assert_eq!(app.view_mode, ViewMode::SideBySide);
        press(&mut app, KeyCode::Char('v'));
        assert_eq!(app.view_mode, ViewMode::Diff);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.view_mode, ViewMode::Normal);
        press(&mut app, KeyCode::Char('v'));
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.view_mode, ViewMode::Normal);

        press(&mut app, KeyCode::Char('v'));
        let mut reloaded = loaded_with_flows(&["Alpha"]);
        reloaded.base = None;
        app.apply(reloaded);
        assert_eq!(app.view_mode, ViewMode::Normal);
        press(&mut app, KeyCode::Char('v'));
        assert_eq!(app.view_mode, ViewMode::Normal);
    }

    fn loaded_with_diagram(source: &str) -> Loaded {
        let flow = casemap::flows::parse_flow(
            "checkout.md",
            &format!("# Checkout\n\nA shopper pays.\n\n```mermaid\nflowchart TD\n{source}\n```"),
        );
        let mut entry = entry("Checkout");
        entry.normal = super::super::normal_view(&flow);
        entry.difference = entry.normal.clone();
        entry.flow = flow;
        Loaded {
            flows: vec![entry],
            ..loaded_with_flows(&[])
        }
    }

    #[test]
    fn view_mode_change_moves_a_missing_selection_to_the_first_step() {
        let mut loaded = loaded_with_diagram("pay[Pay] --> receipt[Receive receipt]");
        loaded.flows[0].difference = loaded_with_diagram("pay[Pay]").flows.remove(0).normal;
        let mut app = App::new(loaded, false, true);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_node_id(), Some("receipt"));
        app.set_view_mode(ViewMode::Diff);
        assert_eq!(app.selected_node_id(), Some("pay"));
    }

    #[test]
    fn selection_is_cleared_when_the_step_disappears() {
        let mut app = App::new(
            loaded_with_diagram("pay[Pay] --> receipt[Receive receipt]"),
            false,
            true,
        );
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_step().unwrap().node_index, 1);
        app.apply(loaded_with_diagram("pay[Pay]"));
        assert!(app.selected_step().is_none());
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_step().unwrap().node_index, 0);
    }

    #[test]
    fn reload_keeps_the_same_step_when_node_indices_change() {
        let mut app = App::new(
            loaded_with_diagram("pay[Pay] --> receipt[Receive receipt]"),
            false,
            true,
        );
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        app.apply(loaded_with_diagram(
            "cart[Open cart] --> pay[Pay] --> receipt[Receive receipt]",
        ));
        assert!(
            app.panel_lines()
                .iter()
                .any(|line| line == "Receive receipt")
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
