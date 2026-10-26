use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph, Wrap};
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
        let empty = app.flows.is_empty();
        let text = if empty {
            Text::raw(welcome_text(&app.catalog))
        } else {
            app.diagram()
                .map(|view| view.text.clone())
                .unwrap_or_default()
        };
        let title = if app.diff_mode && !empty {
            " Diff "
        } else {
            " Diagram "
        };
        let mut diagram = Paragraph::new(text)
            .block(border(
                title,
                app.ascii,
                app.focus == Focus::Diagram,
                app.color,
            ))
            .scroll((app.pan.rows, app.pan.columns));
        if empty {
            diagram = diagram.wrap(Wrap { trim: false });
        }
        frame.render_widget(diagram, areas.diagram);
    }
    if !areas.panel.is_empty() {
        frame.render_widget(
            Paragraph::new(
                visible_panel_lines(app, areas.panel.height.saturating_sub(2)).join("\n"),
            )
            .block(border(" Problems & code ", app.ascii, false, app.color)),
            areas.panel,
        );
    }
    if areas.diagram.height <= 2
        && (app.focus == Focus::Diagram || area.width >= super::layout::WIDE_WIDTH)
    {
        let available = Rect::new(
            areas.header.x,
            area.y,
            areas.header.width,
            area.height.saturating_sub(1),
        );
        frame.render_widget(Clear, available);
        frame.render_widget(
            Paragraph::new(truncate(
                "terminal too small",
                usize::from(available.width),
                app.ascii,
            ))
            .alignment(ratatui::layout::Alignment::Center),
            available,
        );
    }
    draw_status(frame, app, areas.status);
    if app.help {
        draw_help(frame, app, area);
    }
}

fn welcome_text(catalog: &str) -> String {
    let prompt = crate::onboard::agent_prompt(catalog);
    format!(
        "No use cases yet.\n\ncasemap keeps one Mermaid flow per use case in {catalog}/,\nwritten by your coding agent.\n\nQuit (q) and run `casemap` to let Claude Code or Codex write them,\nor ask your agent:\n  \"{prompt}\""
    )
}

fn visible_panel_lines(app: &App, height: u16) -> Vec<String> {
    let mut lines = app.panel_lines();
    let height = usize::from(height);
    if height == 0 {
        lines.clear();
    } else if lines.len() > height {
        let hidden = lines.len() - height + 1;
        lines.truncate(height - 1);
        let ellipsis = if app.ascii { "..." } else { "…" };
        lines.push(format!("{ellipsis} (+{hidden} more)"));
    }
    lines
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
    let items: Vec<_> = if app.flows.is_empty() {
        vec![ListItem::new("no flows yet")]
    } else if app.view.is_empty() {
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
                .take(casemap::flows::SHORT_SHA_LEN)
                .collect::<String>()
        ),
        None => "no git".into(),
    };
    let [left_area, right_area] = ratatui::layout::Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(super::app::dimension(right.width())),
    ])
    .areas(area);
    let mut left = match app.status_text() {
        Some(message) => message.to_owned(),
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
    if app.reloading {
        left = if app.ascii {
            "reloading...".into()
        } else {
            "reloading…".into()
        };
    }
    if !app.flows.is_empty() && app.catalog_problems > 0 {
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
Home                  First flow                 Reset pan
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
    use casemap::{Options, anchors, check, flows};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

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
        let report = check::check_catalog_with(dir, &mut |_| true).unwrap();
        let base = catalog_with_base(fixtures, &current);
        let loaded =
            crate::browse::build_loaded(dir, current, report, "main", Some(&base), options);
        let mut app = App::new(loaded, options.ascii, options.color);
        select_flow(&mut app, "pay-for-an-order");
        app.resize(120, 30);
        app
    }
    fn catalog_with_base(mut old: Vec<flows::Flow>, current: &[flows::Flow]) -> BaseChanges {
        old.extend(flows::load_dir(Path::new("tests/fixtures/flows/before")).unwrap());
        let pay = current
            .iter()
            .find(|flow| flow.name == "pay-for-an-order")
            .unwrap()
            .clone();
        BaseChanges {
            base: "1234567890abcdef".into(),
            changed: vec![],
            root: Default::default(),
            prefix: String::new(),
            changes: flows::compare(&old, current),
            old,
            stale: vec![flows::Stale {
                flow: pay,
                hits: vec![flows::StaleHit {
                    target: anchors::Target::Step("pay".into()),
                    files: vec!["src/pay.rs".into()],
                }],
            }],
        }
    }

    fn select_flow(app: &mut App, name: &str) {
        let selected = app
            .view
            .iter()
            .position(|index| app.flows[*index].flow.name == name)
            .unwrap();
        app.list_state.select(Some(selected));
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle(KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn render_terminal(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
        app.resize(width, height);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
    }

    fn render(app: &mut App, width: u16, height: u16) -> String {
        render_terminal(app, width, height).backend().to_string()
    }

    fn assert_browse_snapshot(name: &str, rendered: String) {
        insta::with_settings!({
            snapshot_path => "../../tests/snapshots",
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!(name, rendered);
        });
    }
    #[test]
    fn unanchored_panel_and_overflow_are_visible() {
        for ascii in [false, true] {
            let mut app = fixture_app_with_options(&Options {
                ascii,
                color: false,
                direction: None,
            });
            select_flow(&mut app, "checkout");
            app.panel = true;
            let rendered = render(&mut app, 120, 30);
            assert!(rendered.contains("no anchors"));
            assert!(!rendered.contains("Code:"));
            let selected = app.view[app.list_state.selected().unwrap()];
            app.flows[selected].problems =
                (0..20).map(|number| format!("problem {number}")).collect();
            let rendered = render(&mut app, 120, 30);
            let marker = if ascii {
                "... (+15 more)"
            } else {
                "… (+15 more)"
            };
            assert!(rendered.contains(marker), "{rendered}");
        }
    }

    #[test]
    fn tiny_diagram_shows_a_message() {
        let mut app = fixture_app();
        app.focus = Focus::Diagram;
        let rendered = render(&mut app, 79, 2);
        assert!(rendered.contains("terminal too small"), "{rendered}");
    }

    #[test]
    fn empty() {
        let directory = assert_fs::TempDir::new().unwrap();
        let mut loaded = crate::browse::build_loaded(
            directory.path(),
            Vec::new(),
            check::check_catalog_with(directory.path(), &mut |_| true).unwrap(),
            "HEAD",
            None,
            &Options::default(),
        );
        loaded.catalog = "docs/flows".into();
        let mut app = App::new(loaded, false, false);
        let rendered = render(&mut app, 120, 30);
        assert!(rendered.contains("no flows yet"));
        assert!(rendered.contains("No use cases yet."));
        assert!(!rendered.contains("catalog: 1 problem"));
        assert!(rendered.contains("docs/flows/"));
        assert_browse_snapshot("browse__empty", rendered);
    }

    #[test]
    fn wide() {
        let mut app = fixture_app();
        let terminal = render_terminal(&mut app, 120, 30);
        let buffer = terminal.backend().buffer();
        assert_eq!(app.view.len(), 8);
        for (row, index) in app.view.iter().enumerate() {
            let entry = &app.flows[*index];
            let expected = format!("{} {}", entry.marker.symbol(false), entry.flow.title);
            let actual: String = (1..=expected.width() as u16)
                .map(|column| buffer[(column, row as u16 + 1)].symbol())
                .collect();
            assert_eq!(actual, expected, "list row {}", row + 1);
        }
        assert_browse_snapshot("browse__wide", terminal.backend().to_string());
    }
    #[test]
    fn narrow_list() {
        assert_browse_snapshot("browse__narrow_list", render(&mut fixture_app(), 60, 20));
    }
    #[test]
    fn narrow_diagram() {
        let mut app = fixture_app();
        app.resize(60, 20);
        press(&mut app, KeyCode::Enter);
        assert_browse_snapshot("browse__narrow_diagram", render(&mut app, 60, 20));
    }
    #[test]
    fn diff_mode() {
        let mut app = fixture_app();
        select_flow(&mut app, "checkout");
        press(&mut app, KeyCode::Char('d'));
        app.panel = true;
        assert_browse_snapshot("browse__diff_mode", render(&mut app, 120, 30));
    }
    #[test]
    fn problems_panel() {
        let mut app = fixture_app();
        app.list_state.select(Some(0));
        press(&mut app, KeyCode::Char('p'));
        assert_browse_snapshot("browse__problems_panel", render(&mut app, 120, 30));
    }
    #[test]
    fn help() {
        let mut app = fixture_app();
        press(&mut app, KeyCode::Char('?'));
        assert_browse_snapshot("browse__help", render(&mut app, 120, 30));
    }
    #[test]
    fn search() {
        let mut app = fixture_app();
        press(&mut app, KeyCode::Char('/'));
        for letter in "flow".chars() {
            press(&mut app, KeyCode::Char(letter));
        }
        assert_browse_snapshot("browse__search", render(&mut app, 120, 30));
    }
    #[test]
    fn panned() {
        let mut app = fixture_app();
        select_flow(&mut app, "wide-flow");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('L'));
        press(&mut app, KeyCode::Char('J'));
        assert!(app.pan.columns > 0);
        assert_browse_snapshot("browse__panned", render(&mut app, 120, 30));
    }
    #[test]
    fn colour_styles_pin_markers_selection_focus_and_diff() {
        let mut app = fixture_app();
        let terminal = render_terminal(&mut app, 120, 30);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 5)].symbol(), "!");
        assert_eq!(buffer[(1, 5)].fg, Color::Yellow);
        for column in 1..31 {
            assert!(
                buffer[(column, 5)]
                    .modifier
                    .contains(Modifier::REVERSED | Modifier::BOLD),
                "selected cell {column}"
            );
        }
        assert!(buffer[(0, 0)].modifier.contains(Modifier::BOLD));
        press(&mut app, KeyCode::Tab);
        let terminal = render_terminal(&mut app, 120, 30);
        assert!(
            terminal.backend().buffer()[(32, 3)]
                .modifier
                .contains(Modifier::BOLD)
        );
        select_flow(&mut app, "checkout");
        press(&mut app, KeyCode::Char('d'));
        let terminal = render_terminal(&mut app, 120, 30);
        let diagram = layout(app.area, &app).diagram;
        let buffer = terminal.backend().buffer();
        let diagram_has_green = (diagram.y + 1..diagram.bottom() - 1).any(|row| {
            (diagram.x + 1..diagram.right() - 1)
                .any(|column| buffer[(column, row)].fg == Color::Green)
        });
        assert!(
            diagram_has_green,
            "union diagram lost its added-step colour"
        );
    }

    #[test]
    fn draw_never_panics_on_tiny_terminals() {
        for (width, height) in [(0, 0), (1, 1), (10, 3), (80, 3), (79, 2)] {
            for state in ["help", "search typing", "panel+diff", "narrow diagram"] {
                let mut app = fixture_app();
                match state {
                    "help" => app.help = true,
                    "search typing" => app.search = Search::Typing("pay".into()),
                    "panel+diff" => {
                        app.panel = true;
                        app.diff_mode = true;
                    }
                    "narrow diagram" => app.focus = Focus::Diagram,
                    _ => unreachable!(),
                }
                render_terminal(&mut app, width, height);
            }
        }
    }

    #[test]
    fn ascii() {
        let mut app = fixture_app_with_options(&Options {
            ascii: true,
            color: false,
            direction: None,
        });
        app.panel = true;
        let rendered = render(&mut app, 120, 30);
        assert!(rendered.is_ascii());
        assert_browse_snapshot("browse__ascii", rendered);
        select_flow(&mut app, "checkout");
        for focus in [Focus::List, Focus::Diagram] {
            app.focus = focus;
            for diff_mode in [false, true] {
                app.diff_mode = diff_mode;
                assert!(render(&mut app, 60, 20).is_ascii());
                assert!(render(&mut app, 120, 30).is_ascii());
            }
        }
    }

    #[test]
    fn pending_reload_status_survives_keys() {
        for ascii in [false, true] {
            let mut app = fixture_app_with_options(&Options {
                ascii,
                color: false,
                direction: None,
            });
            app.reloading = true;
            press(&mut app, KeyCode::Down);
            let rendered = render(&mut app, 120, 30);
            assert!(rendered.contains(if ascii {
                "reloading..."
            } else {
                "reloading…"
            }));
        }
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
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('z'));
        assert!(render(&mut app, 120, 30).contains("no matching flows"));
    }
}
