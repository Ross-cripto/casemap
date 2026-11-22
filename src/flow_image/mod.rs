mod layout;
mod raster;
mod theme;

use std::borrow::Cow;
use std::path::Path;
use std::sync::LazyLock;

use crate::diff::{Change, unmarked};
use crate::flows::{self, Flow};
use crate::model::{Diagram, LineStyle, Mark, Node, Shape};
use crate::{ParseError, markdown, parser};
use layout::{
    ARROW_SIZE, BADGE_RIGHT, BAND_BORDER_OPACITY, BAND_FILL_OPACITY, CARD_BASELINE,
    CARD_BORDER_OPACITY, CARD_FONT_SIZE, CARD_LINE_HEIGHT, CARD_RADIUS, CARD_TEXT_INSET,
    COMPARISON_ABSENT, COMPARISON_CAPTION, COMPARISON_CAPTION_SIZE, COMPARISON_FALLBACK_WIDTH,
    COMPARISON_HEADER_HEIGHT, Card, Connection, DESCRIPTION_FONT_SIZE, DIMMED_OPACITY, EdgeLabel,
    FontFace::SemiBold, ICON_LEFT, ICON_SIZE, ICON_TOP, ICON_VIEWBOX, JUMP_ICON_BASELINE,
    JUMP_TEXT_INSET, JumpKind, LABEL_FONT_SIZE, Layout, Point, REMOVED_OPACITY, Rect, Route,
    SECTION_FONT_SIZE, SECTION_STATUS_INSET, SECTION_STATUS_LABEL, TITLE_FONT_SIZE,
};

pub use raster::{MONO_FAMILY, RasterError, SANS_FAMILY, outlined_svg, png};
pub use theme::{Color, InvalidColor, Theme};

const ICON_STROKE_WIDTH: &str = "1.75";
const CONNECTION_STROKE_WIDTH: &str = "1.25";
const SELECTED_CARD_STROKE_WIDTH: &str = "2";
const MARKED_CARD_STROKE_WIDTH: &str = "1.5";
const ARROW_WING_SIZE: i64 = ARROW_SIZE as i64 - 2;
const BASE_UNAVAILABLE: &str = "base flow could not be drawn";

fn mono_end_attributes() -> &'static str {
    static ATTRIBUTES: LazyLock<String> =
        LazyLock::new(|| format!("text-anchor=\"end\" font-family=\"{MONO_FAMILY}\""));
    &ATTRIBUTES
}

#[derive(Debug, thiserror::Error)]
pub enum FlowImageError {
    #[error("casemap draws use-case flow files with one Mermaid flowchart block")]
    Format,
    #[error(
        "a use-case flow needs rectangle steps, stadium actors, diamond decisions and solid arrows"
    )]
    DiagramStyle,
    #[error("the flowchart has no steps")]
    Empty,
    #[error(transparent)]
    Parse(#[from] ParseError),
}

pub fn parse_flow(flow: &Flow) -> Result<Diagram, FlowImageError> {
    let source = flow.source.as_deref().ok_or(FlowImageError::Format)?;
    if !flow.single_mermaid_block
        || !markdown::is_markdown_path(Path::new(&flow.path))
        || !has_flow_header(source)
    {
        return Err(FlowImageError::Format);
    }
    let diagram = flows::parse_diagram(source, flow.line_offset)?;
    if diagram.nodes.is_empty() {
        return Err(FlowImageError::Empty);
    }
    if !diagram.nodes.iter().all(|node| is_flow_shape(node.shape))
        || !diagram.edges.iter().all(is_flow_edge)
    {
        return Err(FlowImageError::DiagramStyle);
    }
    Ok(diagram)
}

fn has_flow_header(source: &str) -> bool {
    let Some((_, header)) = parser::first_statement(source) else {
        return false;
    };
    parser::is_header(header)
}

pub(crate) fn is_flow_shape(shape: Shape) -> bool {
    matches!(shape, Shape::Rect | Shape::Stadium | Shape::Diamond)
}

pub(crate) fn is_flow_edge(edge: &crate::model::Edge) -> bool {
    edge.style == LineStyle::Solid && edge.arrow_to && !edge.arrow_from
}

#[derive(Clone, Copy, Default)]
pub struct ImageOptions<'a> {
    pub changes: Option<&'a [Change]>,
    pub selected_step: Option<usize>,
}

struct PreparedImage<'a> {
    diagram: Cow<'a, Diagram>,
    previous_labels: Vec<Option<&'a str>>,
    layout: Layout,
}

fn prepare_image<'a>(
    flow: &Flow,
    diagram: &'a Diagram,
    changes: Option<&'a [Change]>,
    has_summary: bool,
) -> PreparedImage<'a> {
    let diagram = match changes {
        Some(_) => Cow::Owned(unmarked(diagram)),
        None => Cow::Borrowed(diagram),
    };
    let previous_labels = previous_step_labels(&diagram, changes.unwrap_or_default());
    let layout = layout::layout_with_previous(
        &diagram,
        &flow.title,
        flow.description.as_deref(),
        &previous_labels,
        has_summary || changes.is_some_and(|changes| changes.iter().any(is_step_change)),
    );
    PreparedImage {
        diagram,
        previous_labels,
        layout,
    }
}

pub fn svg(flow: &Flow, diagram: &Diagram, theme: &Theme, options: ImageOptions<'_>) -> String {
    render_svg(flow, diagram, theme, options, None)
}

pub fn svg_with_missing_base(
    flow: &Flow,
    diagram: &Diagram,
    theme: &Theme,
    comparison_base: Option<&str>,
) -> String {
    if let Some(base_label) = comparison_base {
        return comparison_svg(
            None,
            Some((flow, diagram)),
            theme,
            base_label,
            Some(BASE_UNAVAILABLE),
        );
    }
    render_svg(
        flow,
        diagram,
        theme,
        ImageOptions::default(),
        Some(BASE_UNAVAILABLE),
    )
}

fn render_svg(
    flow: &Flow,
    diagram: &Diagram,
    theme: &Theme,
    options: ImageOptions<'_>,
    summary: Option<&str>,
) -> String {
    let PreparedImage {
        diagram,
        previous_labels,
        layout,
    } = prepare_image(flow, diagram, options.changes, summary.is_some());
    let mut output = start_svg(&flow.title, layout.width, layout.height, theme);
    let panel = Panel {
        diagram: &diagram,
        layout: &layout,
        theme,
        previous_labels: &previous_labels,
        changes: options.changes.unwrap_or_default(),
        summary,
        selection: options
            .selected_step
            .filter(|&index| index < diagram.nodes.len()),
    };
    panel.draw(&mut output, flow);
    output.push_str("</svg>");
    output
}

pub fn side_by_side_svg(
    before: Option<(&Flow, &Diagram)>,
    after: Option<(&Flow, &Diagram)>,
    theme: &Theme,
    base_label: &str,
) -> String {
    comparison_svg(before, after, theme, base_label, None)
}

fn comparison_svg(
    before: Option<(&Flow, &Diagram)>,
    after: Option<(&Flow, &Diagram)>,
    theme: &Theme,
    base_label: &str,
    summary: Option<&str>,
) -> String {
    let before_layout = comparison_layout(before, false);
    let after_layout = comparison_layout(after, summary.is_some());
    let before_caption = format!("Before · {base_label}");
    let caption_width = layout::text_width(&before_caption, COMPARISON_CAPTION_SIZE, SemiBold)
        .saturating_add(2 * COMPARISON_CAPTION.x);
    let before_width = comparison_width(&before_layout, &after_layout).max(caption_width);
    let after_width = comparison_width(&after_layout, &before_layout);
    let height = before_layout
        .iter()
        .chain(after_layout.iter())
        .map(|layout| layout.height)
        .max()
        .unwrap_or(COMPARISON_HEADER_HEIGHT);
    let height = height + COMPARISON_HEADER_HEIGHT;
    let mut output = start_svg(
        "Flow comparison",
        before_width.saturating_add(after_width),
        height,
        theme,
    );
    draw_comparison_side(
        &mut output,
        before,
        before_layout.as_ref(),
        &before_caption,
        0,
        theme,
        summary.filter(|_| before.is_none()),
    );
    draw_comparison_side(
        &mut output,
        after,
        after_layout.as_ref(),
        if before.is_some() || summary.is_some() {
            "After"
        } else {
            "New"
        },
        before_width,
        theme,
        summary,
    );
    let border = &theme.card_border;
    output.push_str(&format!(
        "<path d=\"M {before_width} 0 V {height}\" stroke=\"{border}\" stroke-width=\"1\"/>"
    ));
    output.push_str("</svg>");
    output
}

fn comparison_layout(side: Option<(&Flow, &Diagram)>, has_summary: bool) -> Option<Layout> {
    side.map(|(flow, diagram)| {
        layout::layout_with_previous(
            diagram,
            &flow.title,
            flow.description.as_deref(),
            &[],
            has_summary,
        )
    })
}

fn comparison_width(side: &Option<Layout>, other: &Option<Layout>) -> u32 {
    side.as_ref()
        .or(other.as_ref())
        .map_or(COMPARISON_FALLBACK_WIDTH, |layout| layout.width)
}

fn draw_comparison_side(
    output: &mut String,
    side: Option<(&Flow, &Diagram)>,
    layout: Option<&Layout>,
    caption: &str,
    left: u32,
    theme: &Theme,
    summary: Option<&str>,
) {
    output.push_str(&format!("<g transform=\"translate({left} 0)\">"));
    text(
        output,
        COMPARISON_CAPTION,
        caption,
        COMPARISON_CAPTION_SIZE,
        &theme.muted,
        "font-weight=\"600\" dominant-baseline=\"text-before-edge\"",
    );
    if let (Some((flow, diagram)), Some(layout)) = (side, layout) {
        output.push_str(&format!(
            "<g transform=\"translate(0 {COMPARISON_HEADER_HEIGHT})\">"
        ));
        Panel {
            diagram,
            layout,
            theme,
            previous_labels: &[],
            changes: &[],
            summary,
            selection: None,
        }
        .draw(output, flow);
        output.push_str("</g>");
    } else {
        text(
            output,
            COMPARISON_ABSENT,
            if summary.is_some() {
                "Flow unavailable"
            } else {
                "Flow absent"
            },
            CARD_FONT_SIZE,
            &theme.muted,
            "",
        );
    }
    output.push_str("</g>");
}

fn previous_step_labels<'a>(diagram: &Diagram, changes: &'a [Change]) -> Vec<Option<&'a str>> {
    diagram
        .nodes
        .iter()
        .map(|node| {
            changes.iter().find_map(|change| match change {
                Change::StepChanged { id, old, new, .. } if id == &node.id && old != new => {
                    Some(old.as_str())
                }
                _ => None,
            })
        })
        .collect()
}

fn is_step_change(change: &Change) -> bool {
    matches!(
        change,
        Change::StepAdded { .. } | Change::StepChanged { .. } | Change::StepRemoved { .. }
    )
}

fn diff_summary(output: &mut String, origin: Point, changes: &[Change], theme: &Theme) {
    let mut added = 0;
    let mut changed = 0;
    let mut removed = 0;
    for change in changes {
        match change {
            Change::StepAdded { .. } => added += 1,
            Change::StepChanged { .. } => changed += 1,
            Change::StepRemoved { .. } => removed += 1,
            Change::LinkAdded { .. } | Change::LinkChanged { .. } | Change::LinkRemoved { .. } => {}
        }
    }
    let Point { x, y } = origin;
    output.push_str(&format!(
        "<text x=\"{x}\" y=\"{y}\" font-size=\"{LABEL_FONT_SIZE}\" font-weight=\"500\">"
    ));
    let parts = [
        (
            added,
            plural(added, "step added", "steps added"),
            &theme.added,
        ),
        (changed, "changed", &theme.changed),
        (removed, "removed", &theme.removed),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, label, color)| format!("<tspan fill=\"{color}\">{count} {label}</tspan>"))
    .collect::<Vec<_>>();
    let separator = format!("<tspan fill=\"{}\"> · </tspan>", theme.muted);
    output.push_str(&parts.join(&separator));
    output.push_str("</text>");
}

fn plural(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

struct Panel<'a> {
    diagram: &'a Diagram,
    layout: &'a Layout,
    theme: &'a Theme,
    previous_labels: &'a [Option<&'a str>],
    changes: &'a [Change],
    summary: Option<&'a str>,
    selection: Option<usize>,
}

impl Panel<'_> {
    fn draw(&self, output: &mut String, flow: &Flow) {
        text(
            output,
            self.layout.title,
            &flow.title,
            TITLE_FONT_SIZE,
            &self.theme.text,
            "font-weight=\"600\"",
        );
        for description in &self.layout.description {
            text(
                output,
                description.origin,
                &description.text,
                DESCRIPTION_FONT_SIZE,
                &self.theme.muted,
                "",
            );
        }
        if let Some(origin) = self.layout.diff_summary {
            if let Some(summary) = self.summary {
                text(
                    output,
                    origin,
                    summary,
                    LABEL_FONT_SIZE,
                    &self.theme.muted,
                    "",
                );
            } else {
                diff_summary(output, origin, self.changes, self.theme);
            }
        }
        let selected_band = self.selected_band();
        let changed_sections = self.changed_sections();
        for (section_index, section) in self.layout.sections.iter().enumerate() {
            let accent = self.theme.section(section.cluster_index);
            start_opacity(output, self.band_is_dimmed(section_index, selected_band));
            rectangle(
                output,
                section.bounds,
                CARD_RADIUS,
                accent,
                accent,
                &format!(
                    "fill-opacity=\"{BAND_FILL_OPACITY}\" stroke-opacity=\"{BAND_BORDER_OPACITY}\""
                ),
            );
            if let (Some(index), Some(origin)) = (section.cluster_index, section.heading) {
                let title = &self.diagram.clusters[index].title;
                text(
                    output,
                    origin,
                    title,
                    SECTION_FONT_SIZE,
                    accent,
                    "font-weight=\"600\"",
                );
                if changed_sections[index] {
                    let origin = Point {
                        x: section.bounds.right().saturating_sub(SECTION_STATUS_INSET),
                        y: origin.y,
                    };
                    text(
                        output,
                        origin,
                        SECTION_STATUS_LABEL,
                        LABEL_FONT_SIZE,
                        &self.theme.changed,
                        mono_end_attributes(),
                    );
                }
            }
            output.push_str("</g>");
        }
        let active = active_paths(self.layout, self.diagram, self.selection);
        for bus in &self.layout.buses {
            start_opacity(output, !active.line_sources[bus.from]);
            line(output, &bus.points, &self.theme.line, false, false);
            output.push_str("</g>");
        }
        for stem in &self.layout.jump_stems {
            self.jump_stem(output, stem, &active.edges);
        }
        for connection in &self.layout.connections {
            self.connection(
                output,
                connection,
                active.edges[connection.edge_index],
                selected_band,
            );
        }
        for card in &self.layout.cards {
            self.card(output, card, &active.cards);
        }
    }

    fn selected_band(&self) -> Option<usize> {
        let selected = self.selection?;
        self.layout
            .cards
            .iter()
            .find(|card| card.node_index == selected)
            .map(|card| card.band_index)
    }

    fn band_is_dimmed(&self, band: usize, selected_band: Option<usize>) -> bool {
        self.selection.is_some() && selected_band != Some(band)
    }

    fn changed_sections(&self) -> Vec<bool> {
        self.diagram
            .clusters
            .iter()
            .enumerate()
            .map(|(index, section)| {
                self.has_step_change(&section.id)
                    || self.has_changed_node(index)
                    || self.has_changed_edge(index)
            })
            .collect()
    }

    fn has_step_change(&self, id: &str) -> bool {
        self.changes.iter().any(|change| match change {
            Change::StepChanged {
                section: Some(section),
                ..
            } => section.previous.as_deref() == Some(id) || section.current.as_deref() == Some(id),
            _ => false,
        })
    }

    fn has_changed_node(&self, index: usize) -> bool {
        self.diagram
            .nodes
            .iter()
            .enumerate()
            .any(|(node_index, node)| {
                self.diagram.node_cluster[node_index] == Some(index) && node.mark != Mark::Same
            })
    }

    fn has_changed_edge(&self, index: usize) -> bool {
        self.layout.connections.iter().any(|connection| {
            self.diagram.node_cluster[connection.from] == Some(index)
                && self.diagram.edges[connection.edge_index].mark != Mark::Same
        })
    }

    fn card(&self, output: &mut String, card: &Card, active: &[bool]) {
        let node = &self.diagram.nodes[card.node_index];
        let accent = self
            .theme
            .section(self.diagram.node_cluster[card.node_index]);
        let selected = self.selection == Some(card.node_index);
        start_opacity(output, !active[card.node_index]);
        if node.mark == Mark::Removed {
            output.push_str(&format!("<g opacity=\"{REMOVED_OPACITY}\">"));
        }
        let border = if selected {
            &self.theme.focus
        } else {
            mark_color(node.mark, self.theme).unwrap_or(accent)
        };
        rectangle(
            output,
            card.bounds,
            CARD_RADIUS,
            &self.theme.card,
            border,
            &border_style(node.mark, selected),
        );
        let icon_left = card.bounds.x.saturating_add(ICON_LEFT);
        let icon_top = card.bounds.y.saturating_add(ICON_TOP);
        let scale = f64::from(ICON_SIZE) / ICON_VIEWBOX;
        let icon = node.effective_icon().svg_body();
        output.push_str(&format!("<g transform=\"translate({icon_left} {icon_top}) scale({scale})\" fill=\"none\" stroke=\"{accent}\" stroke-width=\"{ICON_STROKE_WIDTH}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{icon}</g>"));
        self.draw_card_labels(output, card, node);
        self.draw_badge(output, card.bounds, node.mark);
        if node.mark == Mark::Removed {
            output.push_str("</g>");
        }
        output.push_str("</g>");
    }

    fn draw_card_labels(&self, output: &mut String, card: &Card, node: &Node) {
        let mut baseline = card.bounds.y.saturating_add(CARD_BASELINE);
        let left = card.bounds.x.saturating_add(CARD_TEXT_INSET);
        let decoration = if node.mark == Mark::Removed {
            "text-decoration=\"line-through\""
        } else {
            ""
        };
        for label in &node.label {
            text(
                output,
                Point {
                    x: left,
                    y: baseline,
                },
                label,
                CARD_FONT_SIZE,
                &self.theme.text,
                decoration,
            );
            baseline = baseline.saturating_add(CARD_LINE_HEIGHT);
        }
        if let Some(Some(previous)) = self.previous_labels.get(card.node_index) {
            text(
                output,
                Point {
                    x: left,
                    y: baseline,
                },
                previous,
                LABEL_FONT_SIZE,
                &self.theme.muted,
                "text-decoration=\"line-through\"",
            );
        }
    }

    fn draw_badge(&self, output: &mut String, bounds: Rect, mark: Mark) {
        if let Some(label) = layout::card_badge_label(mark)
            && let Some(color) = mark_color(mark, self.theme)
        {
            let origin = Point {
                x: bounds.right().saturating_sub(BADGE_RIGHT),
                y: bounds.y.saturating_add(CARD_BASELINE),
            };
            text(
                output,
                origin,
                label,
                LABEL_FONT_SIZE,
                color,
                mono_end_attributes(),
            );
        }
    }

    fn jump_stem(&self, output: &mut String, stem: &layout::JumpStem, active: &[bool]) {
        let last_active = stem.rows.iter().rposition(|row| active[row.edge_index]);
        let last_live = stem
            .rows
            .iter()
            .rposition(|row| self.diagram.edges[row.edge_index].mark != Mark::Removed);
        for (position, points) in stem.segments().enumerate() {
            start_opacity(output, last_active.is_none_or(|last| last < position));
            let removed = last_live.is_none_or(|last| last < position);
            let color = if removed {
                &self.theme.removed
            } else {
                self.theme.section(self.diagram.node_cluster[stem.from])
            };
            if removed {
                output.push_str(&format!("<g opacity=\"{REMOVED_OPACITY}\">"));
            }
            line(output, &points, color, removed, false);
            if removed {
                output.push_str("</g>");
            }
            output.push_str("</g>");
        }
    }

    fn connection(
        &self,
        output: &mut String,
        connection: &Connection,
        active: bool,
        selected_band: Option<usize>,
    ) {
        let edge = &self.diagram.edges[connection.edge_index];
        let target_cluster = self.diagram.node_cluster[connection.to];
        let accent = self.theme.section(target_cluster);
        let edge_color = mark_color(edge.mark, self.theme).unwrap_or(accent);
        start_opacity(output, !active);
        let removed = edge.mark == Mark::Removed;
        if removed {
            output.push_str(&format!("<g opacity=\"{REMOVED_OPACITY}\">"));
        }
        match &connection.route {
            Route::Line { points } => line(output, points, edge_color, removed, true),
            Route::Jump {
                points,
                origin,
                kind,
                target,
            } => {
                line(output, points, edge_color, removed, false);
                let jump_color = mark_color(edge.mark, self.theme).unwrap_or(accent);
                jump_arrow(output, *origin, *kind, jump_color);
                jump_text(output, *origin, *kind, target, jump_color);
            }
        }
        if let Some(label) = &connection.label {
            let color = match label.text.to_ascii_lowercase().as_str() {
                "yes" => &self.theme.yes,
                "no" => &self.theme.no,
                _ => match target_cluster {
                    Some(_) => accent,
                    None => &self.theme.text,
                },
            };
            self.draw_label_background(output, label, selected_band);
            let style = if removed {
                "font-weight=\"600\" text-anchor=\"middle\" text-decoration=\"line-through\""
            } else {
                "font-weight=\"600\" text-anchor=\"middle\""
            };
            let center = f64::from(label.bounds.x) + f64::from(label.bounds.width) / 2.0;
            text_at(
                output,
                center,
                label.origin.y,
                &label.text,
                LABEL_FONT_SIZE,
                color,
                style,
            );
        }
        if removed {
            output.push_str("</g>");
        }
        output.push_str("</g>");
    }

    fn draw_label_background(
        &self,
        output: &mut String,
        label: &EdgeLabel,
        selected_band: Option<usize>,
    ) {
        let Rect {
            x,
            y,
            width,
            height,
        } = label.bounds;
        let center = Point {
            x: label.origin.x,
            y: y + height / 2,
        };
        let background = &self.theme.background;
        output.push_str(&format!(
            "<g data-edge-label=\"true\"><svg x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" viewBox=\"{x} {y} {width} {height}\" overflow=\"hidden\">"
        ));
        rectangle(
            output,
            label.bounds,
            0,
            background,
            background,
            "stroke-width=\"0\"",
        );
        for (index, band) in self.layout.sections.iter().enumerate() {
            if band.bounds.contains(center) {
                start_opacity(output, self.band_is_dimmed(index, selected_band));
                let accent = self.theme.section(band.cluster_index);
                rectangle(
                    output,
                    band.bounds,
                    CARD_RADIUS,
                    accent,
                    accent,
                    &format!("fill-opacity=\"{BAND_FILL_OPACITY}\" stroke-width=\"0\""),
                );
                output.push_str("</g>");
            }
        }
        output.push_str("</svg></g>");
    }
}

fn border_style(mark: Mark, selected: bool) -> String {
    let width = if selected {
        SELECTED_CARD_STROKE_WIDTH
    } else {
        MARKED_CARD_STROKE_WIDTH
    };
    if mark == Mark::Removed {
        return format!("stroke-width=\"{width}\" stroke-dasharray=\"5 4\"");
    }
    if selected || mark != Mark::Same {
        return format!("stroke-width=\"{width}\"");
    }
    format!("stroke-opacity=\"{CARD_BORDER_OPACITY}\"")
}

struct ActivePaths {
    cards: Vec<bool>,
    edges: Vec<bool>,
    line_sources: Vec<bool>,
}

fn active_paths(layout: &Layout, diagram: &Diagram, selection: Option<usize>) -> ActivePaths {
    let all_active = selection.is_none();
    let mut active = ActivePaths {
        cards: vec![all_active; diagram.nodes.len()],
        edges: vec![all_active; diagram.edges.len()],
        line_sources: vec![all_active; diagram.nodes.len()],
    };
    let Some(selected) = selection else {
        return active;
    };
    active.cards[selected] = true;
    for connection in &layout.connections {
        if connection.to == selected {
            active.cards[connection.from] = true;
            active.edges[connection.edge_index] = true;
            if matches!(connection.route, Route::Line { .. }) {
                active.line_sources[connection.from] = true;
            }
        } else if back_to_selected_section(connection, diagram.node_cluster[selected], diagram) {
            active.edges[connection.edge_index] = true;
        }
    }
    active
}

fn back_to_selected_section(
    connection: &Connection,
    selected_cluster: Option<usize>,
    diagram: &Diagram,
) -> bool {
    matches!(
        connection.route,
        Route::Jump {
            kind: JumpKind::Back,
            ..
        }
    ) && selected_cluster.is_some()
        && diagram.node_cluster[connection.to] == selected_cluster
}

fn mark_color(mark: Mark, theme: &Theme) -> Option<&Color> {
    match mark {
        Mark::Same => None,
        Mark::Added => Some(&theme.added),
        Mark::Changed => Some(&theme.changed),
        Mark::Removed => Some(&theme.removed),
    }
}

fn start_svg(title: &str, width: u32, height: u32, theme: &Theme) -> String {
    let background = &theme.background;
    let safe_title = escape(title);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" font-family=\"{SANS_FAMILY}\"><title>{safe_title}</title><rect width=\"{width}\" height=\"{height}\" fill=\"{background}\"/>"
    )
}

fn rectangle(
    output: &mut String,
    bounds: Rect,
    radius: u32,
    fill: &Color,
    stroke: &Color,
    attributes: &str,
) {
    let Rect {
        x,
        y,
        width,
        height,
    } = bounds;
    output.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" rx=\"{radius}\" fill=\"{fill}\" stroke=\"{stroke}\" {attributes}/>"));
}

fn text(
    output: &mut String,
    origin: Point,
    text: &str,
    size: u32,
    color: &Color,
    attributes: &str,
) {
    text_at(output, origin.x, origin.y, text, size, color, attributes);
}

fn text_at(
    output: &mut String,
    x: impl std::fmt::Display,
    y: u32,
    text: &str,
    size: u32,
    color: &Color,
    attributes: &str,
) {
    let content = escape(text);
    output.push_str(&format!("<text x=\"{x}\" y=\"{y}\" font-size=\"{size}\" fill=\"{color}\" {attributes}>{content}</text>"));
}

fn start_opacity(output: &mut String, dimmed: bool) {
    if dimmed {
        output.push_str(&format!("<g opacity=\"{DIMMED_OPACITY}\">"));
    } else {
        output.push_str("<g>");
    }
}

fn line(output: &mut String, points: &[Point], color: &Color, dashed: bool, arrow: bool) {
    if points.len() < 2 {
        return;
    }
    let coordinates = points
        .iter()
        .map(|point| format!("{},{}", point.x, point.y))
        .collect::<Vec<_>>()
        .join(" ");
    let dash = if dashed {
        "stroke-dasharray=\"4 3\""
    } else {
        ""
    };
    output.push_str(&format!("<polyline points=\"{coordinates}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{CONNECTION_STROKE_WIDTH}\" stroke-linejoin=\"round\" {dash}/>"));
    if !arrow {
        return;
    }
    let tip = points[points.len() - 1];
    let previous = points[points.len() - 2];
    let horizontal = (i64::from(tip.x) - i64::from(previous.x)).signum();
    let vertical = (i64::from(tip.y) - i64::from(previous.y)).signum();
    let x = i64::from(tip.x);
    let y = i64::from(tip.y);
    let size = i64::from(ARROW_SIZE);
    let base_x = x - size * horizontal;
    let base_y = y - size * vertical;
    let wing_x = ARROW_WING_SIZE * vertical;
    let wing_y = ARROW_WING_SIZE * horizontal;
    output.push_str(&format!(
        "<path d=\"M {} {} L {x} {y} L {} {} Z\" fill=\"{color}\"/>",
        base_x - wing_x,
        base_y - wing_y,
        base_x + wing_x,
        base_y + wing_y
    ));
}

fn jump_arrow(output: &mut String, origin: Point, kind: JumpKind, color: &Color) {
    let Point { x, y } = origin;
    let path = match kind {
        JumpKind::Back => "M12 1v4a3 3 0 0 1-3 3H2m3-3L2 8l3 3",
        JumpKind::Go => "M2 1v4a3 3 0 0 0 3 3h7m-3-3 3 3-3 3",
    };
    let top = y.saturating_sub(JUMP_ICON_BASELINE);
    output.push_str(&format!("<path d=\"{path}\" transform=\"translate({x} {top})\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.3\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>"));
}

fn jump_text(output: &mut String, origin: Point, kind: JumpKind, target: &str, color: &Color) {
    let x = origin.x.saturating_add(JUMP_TEXT_INSET);
    let y = origin.y;
    let prefix = kind.prefix();
    let encoded = escape(target);
    output.push_str(&format!(
        "<text x=\"{x}\" y=\"{y}\" font-size=\"{LABEL_FONT_SIZE}\" fill=\"{color}\"><tspan font-weight=\"600\">{prefix}{encoded}</tspan></text>"
    ));
}

pub fn size(svg: &str) -> (f64, f64) {
    let Some((header, _)) = svg.split_once('>') else {
        return (0.0, 0.0);
    };
    (
        svg_dimension(header, "width"),
        svg_dimension(header, "height"),
    )
}

fn svg_dimension(header: &str, attribute: &str) -> f64 {
    header
        .split_once(&format!(" {attribute}=\""))
        .and_then(|(_, value)| value.split_once('\"'))
        .and_then(|(value, _)| value.parse::<f64>().ok())
        .filter(|dimension| dimension.is_finite() && *dimension > 0.0)
        .unwrap_or(0.0)
}

pub fn crop(svg: &str, x: f64, y: f64, width: f64, height: f64) -> String {
    let Some((_, content)) = svg.split_once('>') else {
        return svg.to_owned();
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.2}\" height=\"{height:.2}\" viewBox=\"{x:.2} {y:.2} {width:.2} {height:.2}\" role=\"img\" font-family=\"{SANS_FAMILY}\">{content}"
    )
}

fn escape(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t' | '\n' | '\r' => escaped.push(character),
            character
                if !character.is_control() && !matches!(character, '\u{fffe}' | '\u{ffff}') =>
            {
                escaped.push(character)
            }
            _ => {}
        }
    }
    escaped
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepPosition {
    pub node_index: usize,
    pub top: u32,
    pub bottom: u32,
    pub returns_from: Vec<usize>,
}

pub fn steps(flow: &Flow, diagram: &Diagram, options: ImageOptions<'_>) -> Vec<StepPosition> {
    let PreparedImage { layout, .. } = prepare_image(flow, diagram, options.changes, false);
    let mut returns = vec![Vec::new(); diagram.nodes.len()];
    for connection in layout.connections {
        if matches!(
            connection.route,
            Route::Jump {
                kind: JumpKind::Back,
                ..
            }
        ) && !returns[connection.to].contains(&connection.from)
        {
            returns[connection.to].push(connection.from);
        }
    }
    let mut positions = Vec::with_capacity(layout.cards.len());
    for card in layout.cards {
        positions.push(StepPosition {
            node_index: card.node_index,
            top: card.bounds.y,
            bottom: card.bounds.y + card.bounds.height,
            returns_from: std::mem::take(&mut returns[card.node_index]),
        });
    }
    positions
}

#[cfg(test)]
mod tests {
    use resvg::{tiny_skia, usvg::roxmltree::Document};

    use super::*;

    #[test]
    fn label_boxes_match_the_band_behind_them() {
        let flow = flows::parse_flow(
            "sections.md",
            "# Sections\n\n```mermaid\nflowchart TD
subgraph source [Source]
    choice{Proceed?} -->|yes| work[Continue] --> finish[Finish]
end
subgraph side [Side]
    error[Recover] -->|retry| choice
end
choice -->|no| error
```\n",
        );
        let diagram = parse_flow(&flow).unwrap();
        let prepared = prepare_image(&flow, &diagram, None, false);
        for (theme, selection) in [(Theme::dark(), Some(0)), (Theme::light(), None)] {
            let options = ImageOptions {
                changes: None,
                selected_step: selection,
            };
            let svg = svg(&flow, &diagram, &theme, options);
            let document = Document::parse(&svg).unwrap();
            let bands: Vec<_> = document
                .descendants()
                .filter(|node| node.attribute("stroke-opacity") == Some(BAND_BORDER_OPACITY))
                .collect();
            let label_groups: Vec<_> = document
                .descendants()
                .filter(|node| node.attribute("data-edge-label") == Some("true"))
                .collect();
            let png = tiny_skia::Pixmap::decode_png(&png(&svg, 1.0).unwrap()).unwrap();
            let labels: Vec<_> = prepared
                .layout
                .connections
                .iter()
                .filter_map(|edge| edge.label.as_ref())
                .collect();
            assert_eq!(label_groups.len(), labels.len());
            assert_eq!(bands.len(), prepared.layout.sections.len());
            let mut saw_nested_label = false;
            for (group, label) in label_groups.into_iter().zip(labels) {
                let bounds = label.bounds;
                let center = Point {
                    x: label.origin.x,
                    y: bounds.y + bounds.height / 2,
                };
                let viewport = group.first_element_child().unwrap();
                assert_eq!(viewport.attribute("overflow"), Some("hidden"));
                let background = theme.background.to_string();
                let mut expected = vec![(Some(background.as_str()), None, None)];
                for (svg_band, band) in bands.iter().zip(&prepared.layout.sections) {
                    if band.bounds.contains(center) {
                        expected.push((
                            svg_band.attribute("fill"),
                            svg_band.attribute("fill-opacity"),
                            svg_band.parent().unwrap().attribute("opacity"),
                        ));
                    }
                }
                saw_nested_label |= expected.len() > 2;
                let layers: Vec<_> = viewport
                    .descendants()
                    .filter(|node| node.has_tag_name("rect"))
                    .map(|node| {
                        (
                            node.attribute("fill"),
                            node.attribute("fill-opacity"),
                            node.parent().unwrap().attribute("opacity"),
                        )
                    })
                    .collect();
                assert_eq!(layers, expected, "{}", label.text);
                let text = group.next_sibling().unwrap();
                assert_eq!(text.attribute("text-anchor"), Some("middle"));
                assert_eq!(
                    text.attribute("x").unwrap().parse::<f64>().unwrap(),
                    f64::from(bounds.x) + f64::from(bounds.width) / 2.0,
                );
                assert_eq!(
                    png.pixel(bounds.x + 1, bounds.y + 1),
                    png.pixel(bounds.x + 1, bounds.y - 1),
                    "{} has a visible box",
                    label.text,
                );
            }
            assert!(
                saw_nested_label,
                "the fixture must exercise composited bands"
            );
        }
    }
}
