use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Focus, Marker, Search};
use super::layout::layout;

const STATUS_KEYS: &str = "↑↓ select · Tab focus · / search · d diff · p panel · ? help · q quit";
const ASCII_STATUS_KEYS: &str =
    "up/down select | Tab focus | / search | d diff | p panel | ? help | q quit";
const ASCII_BORDER: symbols::border::Set = symbols::border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

pub(super) fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    app.resize(area.width, area.height);
    let areas = layout(area, app);
    if !areas.list.is_empty() {
        draw_list(frame, app, areas.list);
    }
    if !areas.diagram.is_empty() {
        draw_header(frame, app, areas.header);
        let text = app
            .diagram()
            .map(|view| view.text.clone())
            .unwrap_or_default();
        let title = if app.diff_mode { " Diff " } else { " Diagram " };
        let diagram = Paragraph::new(text)
            .block(border(
                title,
                app.ascii,
                app.focus == Focus::Diagram,
                app.color,
            ))
            .scroll(app.pan);
        frame.render_widget(diagram, areas.diagram);
    }
    if !areas.panel.is_empty() {
        frame.render_widget(
            Paragraph::new(app.panel_lines().join("\n")).block(border(
                " Problems & code ",
                app.ascii,
                false,
                app.color,
            )),
            areas.panel,
        );
    }
    draw_status(frame, app, areas.status);
    if app.help {
        draw_help(frame, app, area);
    }
}

fn border(title: &str, ascii: bool, focused: bool, color: bool) -> Block<'_> {
    let mut block = Block::bordered()
        .title(title)
        .border_type(BorderType::Plain);
    if ascii {
        block = block.border_set(ASCII_BORDER);
    }
    if focused {
        let mut style = Style::default().add_modifier(Modifier::BOLD);
        if color {
            style = style.fg(Color::Yellow);
        }
        block = block.border_style(style);
    }
    block
}

fn marker_style(marker: Marker, color: bool) -> Style {
    if !color {
        return Style::default();
    }
    let color = match marker {
        Marker::Error | Marker::Removed => Color::Red,
        Marker::Stale | Marker::Modified => Color::Yellow,
        Marker::New => Color::Green,
        Marker::Same => Color::DarkGray,
    };
    Style::default().fg(color)
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect) {
    let title_width = usize::from(area.width.saturating_sub(4));
    let items: Vec<_> = if app.view.is_empty() {
        vec![ListItem::new("no matching flows")]
    } else {
        app.view
            .iter()
            .map(|index| {
                let entry = &app.flows[*index];
                ListItem::new(Line::from(vec![
                    Span::styled(
                        entry.marker.symbol(app.ascii),
                        marker_style(entry.marker, app.color),
                    ),
                    Span::raw(" "),
                    Span::raw(truncate(&entry.flow.title, title_width, app.ascii)),
                ]))
            })
            .collect()
    };
    let list = List::new(items)
        .block(border(
            " Flows ",
            app.ascii,
            app.focus == Focus::List,
            app.color,
        ))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));
    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn truncate(title: &str, width: usize, ascii: bool) -> String {
    if title.width() <= width {
        return title.to_owned();
    }
    let suffix = if ascii { "..." } else { "…" };
    if width < suffix.width() {
        return suffix.chars().take(width).collect();
    }
    let limit = width - suffix.width();
    let mut text = String::new();
    let mut used = 0;
    for character in title.chars() {
        let columns = character.width().unwrap_or(0);
        if used + columns > limit {
            break;
        }
        used += columns;
        text.push(character);
    }
    text.push_str(suffix);
    text
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let Some(entry) = app.selected() else { return };
    let mut lines = vec![
        Line::styled(
            entry.flow.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(entry.path.clone()),
        Line::raw(entry.flow.description.clone().unwrap_or_default()),
    ];
    if app.diff_mode {
        lines.push(Line::raw(entry.difference.summary.clone()));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let right = match &app.base {
        Some(base) => format!(
            "base {} ({})",
            base.reference,
            base.sha
                .chars()
                .take(strata::flows::SHORT_SHA_LEN)
                .collect::<String>()
        ),
        None => "no git".into(),
    };
    let [left_area, right_area] = ratatui::layout::Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(super::app::dimension(right.width())),
    ])
    .areas(area);
    let mut left = match &app.status {
        Some(message) => message.clone(),
        None => match &app.search {
            Search::Typing(query) => format!("/{query}"),
            Search::Kept(query) => format!("/{query} (Esc clears)"),
            Search::Off => {
                if app.ascii {
                    ASCII_STATUS_KEYS.into()
                } else {
                    STATUS_KEYS.into()
                }
            }
        },
    };
    if app.catalog_problems > 0 {
        let suffix = if app.catalog_problems == 1 {
            "problem"
        } else {
            "problems"
        };
        left = format!("catalog: {} {suffix} | {left}", app.catalog_problems);
    }
    frame.render_widget(Paragraph::new(left.clone()), left_area);
    frame.render_widget(
        Paragraph::new(right).alignment(ratatui::layout::Alignment::Right),
        right_area,
    );
    if matches!(app.search, Search::Typing(_)) && !app.help && !left_area.is_empty() {
        let cursor = super::app::dimension(left.width()).min(left_area.width.saturating_sub(1));
        frame.set_cursor_position((left_area.x.saturating_add(cursor), left_area.y));
    }
}

fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let overlay = area.centered(Constraint::Length(76), Constraint::Length(19));
    let help = "Key                   List                       Diagram
up/down, j/k          Select a flow              Pan one row
left/right, h/l                                  Pan one column
H/J/K/L                                          Pan ten columns/rows
PgUp/PgDn             Select by page             Pan by page
Home                  First flow                Reset pan
Tab                   Focus diagram              Focus list
Enter                 Open diagram (narrow)
/                     Search title or name
Enter (search)        Keep filter (empty clears)
Esc                   Help > search > filter > narrow list > quit
up/down (search)      Select; other printable keys enter the query
d                     Toggle diff mode
p                     Toggle problems and code panel
r                     Reload flows, checks and anchored code
?                     Toggle this help
q / Ctrl-C            Quit (Ctrl-C also works during search)";
    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Paragraph::new(help).block(border(" Help ", app.ascii, false, app.color)),
        overlay,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BaseChanges;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;
    use strata::{Options, anchors, check, flows};

    fn fixture_app() -> App {
        fixture_app_with_options(&Options {
            color: true,
            ..Options::default()
        })
    }

    fn fixture_app_with_options(options: &Options) -> App {
        let dir = Path::new("tests/fixtures/flows/browse");
        let fixtures = flows::load_dir(dir).unwrap();
        let mut current = fixtures.clone();
        current.extend(flows::load_dir(Path::new("tests/fixtures/flows/after")).unwrap());
        current.sort_by(|left, right| left.path.cmp(&right.path));
        let mut old = fixtures;
        old.extend(flows::load_dir(Path::new("tests/fixtures/flows/before")).unwrap());
        let report = check::check_catalog_with(dir, &mut |_| true).unwrap();
        let pay = current
            .iter()
            .find(|flow| flow.name == "pay-for-an-order")
            .unwrap()
            .clone();
        let base = BaseChanges {
            base: "1234567890abcdef".into(),
            changes: flows::compare(&old, &current),
            old,
            stale: vec![flows::Stale {
                flow: pay,
                hits: vec![flows::StaleHit {
                    target: anchors::Target::Step("pay".into()),
                    files: vec!["src/pay.rs".into()],
                }],
            }],
        };
        let loaded =
            crate::browse::build_loaded(dir, current, report, "main", Some(&base), options);
        let mut app = App::new(loaded, options.ascii, options.color);
        let selected = app
            .view
            .iter()
            .position(|index| app.flows[*index].flow.name == "pay-for-an-order")
            .unwrap();
        app.list_state.select(Some(selected));
        app.resize(120, 30);
        app
    }
    fn key(app: &mut App, code: KeyCode) {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn render(app: &mut App, width: u16, height: u16) -> String {
        app.resize(width, height);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().to_string()
    }
    fn snapshot(name: &str, app: &mut App, width: u16, height: u16) {
        let rendered = render(app, width, height);
        insta::with_settings!({ snapshot_path => "../../tests/snapshots", prepend_module_to_snapshot => false }, { insta::assert_snapshot!(name, rendered); });
    }
    #[test]
    fn wide() {
        let mut app = fixture_app();
        let rendered = render(&mut app, 120, 30);
        for marker in ["✗", "!", "~", "+", "-", "·"] {
            assert!(rendered.contains(marker), "missing {marker}");
        }
        snapshot("browse__wide", &mut app, 120, 30);
    }
    #[test]
    fn narrow_list() {
        snapshot("browse__narrow_list", &mut fixture_app(), 60, 20);
    }
    #[test]
    fn narrow_diagram() {
        let mut app = fixture_app();
        app.resize(60, 20);
        key(&mut app, KeyCode::Enter);
        snapshot("browse__narrow_diagram", &mut app, 60, 20);
    }
    #[test]
    fn diff_mode() {
        let mut app = fixture_app();
        app.list_state.select(
            app.view
                .iter()
                .position(|index| app.flows[*index].flow.name == "checkout"),
        );
        key(&mut app, KeyCode::Char('d'));
        app.panel = true;
        snapshot("browse__diff_mode", &mut app, 120, 30);
    }
    #[test]
    fn problems_panel() {
        let mut app = fixture_app();
        app.list_state.select(Some(0));
        key(&mut app, KeyCode::Char('p'));
        snapshot("browse__problems_panel", &mut app, 120, 30);
    }
    #[test]
    fn help() {
        let mut app = fixture_app();
        key(&mut app, KeyCode::Char('?'));
        snapshot("browse__help", &mut app, 120, 30);
    }
    #[test]
    fn search() {
        let mut app = fixture_app();
        key(&mut app, KeyCode::Char('/'));
        for letter in "flow".chars() {
            key(&mut app, KeyCode::Char(letter));
        }
        snapshot("browse__search", &mut app, 120, 30);
    }
    #[test]
    fn panned() {
        let mut app = fixture_app();
        app.list_state.select(
            app.view
                .iter()
                .position(|index| app.flows[*index].flow.name == "wide-flow"),
        );
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Char('L'));
        key(&mut app, KeyCode::Char('J'));
        assert!(app.pan.1 > 0);
        snapshot("browse__panned", &mut app, 120, 30);
    }
    #[test]
    fn plain_ascii_ui_uses_only_plain_bold_and_reversed() {
        let mut app = fixture_app_with_options(&Options {
            ascii: true,
            color: false,
            direction: None,
        });
        app.catalog_problems = 2;
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        for help in [false, true] {
            app.help = help;
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            assert!(
                terminal
                    .backend()
                    .to_string()
                    .contains("catalog: 2 problems")
            );
            for cell in &buffer.content {
                assert_eq!(cell.fg, Color::Reset);
                assert_eq!(cell.bg, Color::Reset);
                assert!((cell.modifier - (Modifier::BOLD | Modifier::REVERSED)).is_empty());
                assert!(cell.symbol().is_ascii());
            }
        }
    }
    #[test]
    fn titles_fit_and_empty_filter_has_a_message() {
        assert_eq!(truncate("Customer pays", 8, false), "Custome…");
        assert_eq!(truncate("Customer pays", 8, true), "Custo...");
        assert_eq!(truncate("你好世界", 5, false), "你好…");
        let mut app = fixture_app();
        key(&mut app, KeyCode::Char('/'));
        key(&mut app, KeyCode::Char('z'));
        assert!(render(&mut app, 120, 30).contains("no matching flows"));
    }
}
