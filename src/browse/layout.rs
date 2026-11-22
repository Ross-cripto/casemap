use super::app::{App, Focus, ViewMode};
use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Default, Debug)]
pub(super) struct Areas {
    pub list: Rect,
    pub header: Rect,
    pub diagram: Rect,
    pub panel: Rect,
    pub status: Rect,
}

pub(super) fn layout(area: Rect, app: &App) -> Areas {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    let mut areas = Areas {
        status,
        ..Areas::default()
    };
    if app.focus == Focus::List {
        areas.list = body;
        return areas;
    }
    let header_height = if app.view_mode == ViewMode::Normal {
        3
    } else {
        4
    };
    let panel_height = if app.panel {
        super::app::dimension(app.panel_line_count().saturating_add(2)).min(body.height / 3)
    } else {
        0
    };
    let [header, diagram, panel] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Fill(1),
        Constraint::Length(panel_height),
    ])
    .areas(body);
    areas.header = header;
    areas.diagram = diagram;
    areas.panel = panel;
    areas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::app::Loaded;
    #[test]
    fn layout_tracks_terminal_size_and_panel() {
        let mut app = App::new(
            Loaded {
                catalog: "docs/flows".into(),
                has_map: false,
                flows: vec![],
                base: None,
                catalog_problems: 0,
                status: None,
            },
            false,
            false,
        );
        let wide = layout(Rect::new(0, 0, 120, 30), &app);
        assert_eq!(wide.list.width, 120);
        assert_eq!(wide.status, Rect::new(0, 29, 120, 1));
        assert!(wide.diagram.is_empty());
        app.focus = Focus::Diagram;
        let wide = layout(Rect::new(0, 0, 120, 30), &app);
        assert_eq!(wide.header.height, 3);
        assert_eq!(wide.diagram.height, 26);
        app.view_mode = ViewMode::Diff;
        app.panel = true;
        let wide = layout(Rect::new(0, 0, 120, 30), &app);
        assert_eq!(wide.header.height, 4);
        assert!(wide.panel.height <= 29 / 3);
        app.focus = Focus::List;
        let narrow = layout(Rect::new(0, 0, 60, 20), &app);
        assert_eq!(narrow.list.width, 60);
        assert_eq!(narrow.diagram, Rect::default());
        app.focus = Focus::Diagram;
        let narrow = layout(Rect::new(0, 0, 60, 20), &app);
        assert_eq!(narrow.list, Rect::default());
        assert_eq!(narrow.diagram.width, 60);
        for width in 0..3 {
            for height in 0..3 {
                layout(Rect::new(0, 0, width, height), &app);
            }
        }
    }
}
