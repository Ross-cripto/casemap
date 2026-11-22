use std::sync::OnceLock;

use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

use self::FontFace::{Mono, Regular, SemiBold};
use super::raster::{GEIST_MONO, GEIST_REGULAR, GEIST_SEMIBOLD};

use crate::model::{Diagram, Mark, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Point {
    pub(super) x: u32,
    pub(super) y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Rect {
    pub(super) fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub(super) fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub(super) fn contains(self, point: Point) -> bool {
        (self.x..self.right()).contains(&point.x) && (self.y..self.bottom()).contains(&point.y)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) title: Point,
    pub(super) description: Vec<PositionedLabel>,
    pub(super) sections: Vec<Section>,
    pub(super) cards: Vec<Card>,
    pub(super) connections: Vec<Connection>,
    pub(super) buses: Vec<Bus>,
    pub(super) jump_stems: Vec<JumpStem>,
    pub(super) diff_summary: Option<Point>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Section {
    pub(super) cluster_index: Option<usize>,
    pub(super) bounds: Rect,
    pub(super) heading: Option<Point>,
    parent: Option<usize>,
    depth: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Card {
    pub(super) node_index: usize,
    pub(super) band_index: usize,
    pub(super) bounds: Rect,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Connection {
    pub(super) edge_index: usize,
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) label: Option<EdgeLabel>,
    pub(super) route: Route,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PositionedLabel {
    pub(super) text: String,
    pub(super) origin: Point,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct EdgeLabel {
    pub(super) text: String,
    pub(super) origin: Point,
    pub(super) bounds: Rect,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Bus {
    pub(super) from: usize,
    pub(super) points: [Point; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct JumpStem {
    pub(super) from: usize,
    start: Point,
    pub(super) rows: Vec<JumpRow>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct JumpRow {
    pub(super) edge_index: usize,
    point: Point,
}

impl JumpStem {
    pub(super) fn segments(&self) -> impl Iterator<Item = [Point; 2]> {
        let starts = std::iter::once(self.start).chain(self.rows.iter().map(|row| row.point));
        starts
            .zip(&self.rows)
            .map(|(start, row)| [start, row.point])
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Route {
    Line {
        points: Vec<Point>,
    },
    Jump {
        points: Vec<Point>,
        origin: Point,
        kind: JumpKind,
        target: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JumpKind {
    Back,
    Go,
}

pub(super) const TITLE_FONT_SIZE: u32 = 30;
pub(super) const DESCRIPTION_FONT_SIZE: u32 = 15;
pub(super) const SECTION_FONT_SIZE: u32 = 14;
pub(super) const SECTION_STATUS_LABEL: &str = "Changed";
pub(super) const SECTION_STATUS_INSET: u32 = 18;
pub(super) const CARD_FONT_SIZE: u32 = 16;
pub(super) const LABEL_FONT_SIZE: u32 = 13;
const LABEL_CLEARANCE: u32 = 8;
pub(super) const CARD_LINE_HEIGHT: u32 = 22;
pub(super) const CARD_TEXT_INSET: u32 = 54;
pub(super) const CARD_BASELINE: u32 = 29;
pub(super) const CARD_RADIUS: u32 = 9;
pub(super) const ICON_SIZE: u32 = 20;
pub(super) const ICON_LEFT: u32 = 17;
pub(super) const ICON_TOP: u32 = 16;
pub(super) const ICON_VIEWBOX: f64 = 24.0;
pub(super) const BADGE_RIGHT: u32 = 14;
pub(super) const JUMP_TEXT_INSET: u32 = 20;
pub(super) const JUMP_ICON_BASELINE: u32 = 11;
pub(super) const ARROW_SIZE: u32 = 7;
pub(super) const BAND_FILL_OPACITY: &str = ".07";
pub(super) const BAND_BORDER_OPACITY: &str = ".28";
pub(super) const DIMMED_OPACITY: &str = ".40";
pub(super) const REMOVED_OPACITY: &str = ".60";
pub(super) const CARD_BORDER_OPACITY: &str = ".48";
pub(super) const COMPARISON_HEADER_HEIGHT: u32 = 30;
pub(super) const COMPARISON_FALLBACK_WIDTH: u32 = 320;
pub(super) const COMPARISON_CAPTION: Point = Point { x: 48, y: 10 };
pub(super) const COMPARISON_ABSENT: Point = Point { x: 48, y: 74 };
pub(super) const COMPARISON_CAPTION_SIZE: u32 = 12;

const CANVAS_INSET: u32 = 48;
const MIN_CANVAS_WIDTH: u32 = 580;
const MAX_DESCRIPTION_WIDTH: u32 = 820;
const TITLE_BASELINE: u32 = 36;
const DESCRIPTION_BASELINE_GAP: u32 = 30;
const DESCRIPTION_LINE_HEIGHT: u32 = 22;
const HEADER_BOTTOM: u32 = 24;
const CONTENT_TOP: u32 = 112;
const DIFF_SUMMARY_GAP: u32 = 28;
const SECTION_GAP: u32 = 28;
const SECTION_INSET: u32 = 24;
const SECTION_LEFT_INSET: u32 = 32;
const SECTION_HEADING_HEIGHT: u32 = 40;
const SECTION_HEADING_BASELINE: u32 = 17;
const SECTION_STATUS_GAP: u32 = 16;
const CARD_WIDTH: u32 = 342;
const CARD_HEIGHT: u32 = 48;
const CARD_RIGHT_INSET: u32 = 20;
const CARD_BADGE_GAP: u32 = 16;
const CARD_GAP: u32 = 40;
const LABELLED_CARD_GAP: u32 = 52;
const SPINE_INSET: u32 = ICON_LEFT + ICON_SIZE / 2;
const BRANCH_INDENT: u32 = 128;
const DOMINANT_BRANCH_RATIO: usize = 2;
const EDGE_ROW_HEIGHT: u32 = 26;
const EDGE_LABEL_INSET: u32 = 7;
const STUB_LABEL_LINE: u32 = 20;
const STUB_LABEL_PADDING: u32 = 2 * (EDGE_LABEL_INSET + STUB_LABEL_LINE);
const EDGE_LABEL_HEIGHT: u32 = 18;
const LABEL_BASELINE_OFFSET: u32 = 5;
const VERTICAL_LABEL_INSET: u32 = 8;
const STUB_ARROW_SPACE: u32 = 10;
const JUMP_LINE_LENGTH: u32 = 28;
const JUMP_LABEL_LINE_LENGTH: u32 = 70;
const JUMP_ELBOW: u32 = 12;
const JUMP_CONTENT_GAP: u32 = 6;
const JUMP_LABEL_MARGIN: u32 = 10;
const JUMP_LABEL_PADDING: u32 = 2 * (EDGE_LABEL_INSET + JUMP_LABEL_MARGIN);

#[cfg(test)]
pub(super) fn layout(diagram: &Diagram, title: &str, description: Option<&str>) -> Layout {
    layout_with_previous(diagram, title, description, &[], false)
}

pub(super) fn layout_with_previous(
    diagram: &Diagram,
    title: &str,
    description: Option<&str>,
    previous_labels: &[Option<&str>],
    summary: bool,
) -> Layout {
    let graph = Graph::new(diagram);
    let forest = graph.walk();
    let sections = Sections::new(diagram, &forest, &graph.links);
    let minimum_width = MIN_CANVAS_WIDTH
        .max(text_width(title, TITLE_FONT_SIZE, SemiBold).saturating_add(2 * CANVAS_INSET));
    let mut header = Header::new(description, minimum_width, summary);
    let place_content = |top| {
        let image = Layout {
            width: minimum_width,
            height: top,
            title: header.title,
            description: Vec::new(),
            sections: Vec::new(),
            cards: Vec::with_capacity(diagram.nodes.len()),
            connections: Vec::with_capacity(graph.links.len()),
            buses: Vec::new(),
            jump_stems: Vec::new(),
            diff_summary: None,
        };
        Placer {
            graph: &graph,
            forest: &forest,
            sections: &sections,
            previous_labels,
            image,
            titled: vec![false; diagram.clusters.len()],
            neutral_band: None,
            marked: diagram.nodes.iter().any(|node| node.mark != Mark::Same)
                || diagram.edges.iter().any(|edge| edge.mark != Mark::Same),
        }
        .place()
    };
    let mut image = place_content(header.top);
    let wrapped_header = Header::new(description, image.width, summary);
    if header.top != wrapped_header.top {
        image = place_content(wrapped_header.top);
    }
    header = wrapped_header;
    image.width = image.width.max(header.width);
    for band in &mut image.sections {
        let right_inset = CANVAS_INSET + band.depth * SECTION_INSET;
        band.bounds.width = image
            .width
            .saturating_sub(right_inset)
            .saturating_sub(band.bounds.x);
    }
    image.description = header.description;
    image.diff_summary = header.summary;
    image
}

struct Header {
    title: Point,
    description: Vec<PositionedLabel>,
    summary: Option<Point>,
    width: u32,
    top: u32,
}

impl Header {
    fn new(description: Option<&str>, content_width: u32, summary: bool) -> Self {
        let title = Point {
            x: CANVAS_INSET,
            y: TITLE_BASELINE,
        };
        let wrap_width = MAX_DESCRIPTION_WIDTH.max(content_width - 2 * CANVAS_INSET);
        let mut width = content_width;
        let mut baseline = TITLE_BASELINE;
        let mut lines = Vec::new();
        if let Some(description) = description {
            for line in wrap_description(description, wrap_width) {
                baseline += if lines.is_empty() {
                    DESCRIPTION_BASELINE_GAP
                } else {
                    DESCRIPTION_LINE_HEIGHT
                };
                width = width.max(
                    text_width(&line, DESCRIPTION_FONT_SIZE, Regular)
                        .saturating_add(2 * CANVAS_INSET),
                );
                lines.push(PositionedLabel {
                    text: line,
                    origin: Point {
                        x: CANVAS_INSET,
                        y: baseline,
                    },
                });
            }
        }

        let summary = summary.then_some(Point {
            x: CANVAS_INSET,
            y: baseline + DIFF_SUMMARY_GAP,
        });
        let mut top = CONTENT_TOP.max(baseline + HEADER_BOTTOM);
        if let Some(origin) = summary {
            top = (CONTENT_TOP + HEADER_BOTTOM).max(origin.y + HEADER_BOTTOM);
        }
        Self {
            title,
            description: lines,
            summary,
            width,
            top,
        }
    }
}

fn wrap_description(description: &str, wrap_width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0.0;
    let space_width = text_advance(" ", DESCRIPTION_FONT_SIZE, Regular);
    for word in description.split_whitespace() {
        let word_width = text_advance(word, DESCRIPTION_FONT_SIZE, Regular);
        if line.is_empty() {
            line.push_str(word);
            line_width = word_width;
            continue;
        }
        let next_width = line_width + space_width + word_width;
        if next_width > f64::from(wrap_width) {
            lines.push(std::mem::take(&mut line));
            line.push_str(word);
            line_width = word_width;
        } else {
            line.push(' ');
            line.push_str(word);
            line_width = next_width;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[derive(Clone, Copy)]
pub(super) enum FontFace {
    Regular,
    SemiBold,
    Mono,
}

static REGULAR_FONT: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
static SEMIBOLD_FONT: OnceLock<Option<FontRef<'static>>> = OnceLock::new();
static MONO_FONT: OnceLock<Option<FontRef<'static>>> = OnceLock::new();

pub(super) fn text_width(text: &str, font_size: u32, face: FontFace) -> u32 {
    text_advance(text, font_size, face).ceil() as u32
}

fn text_advance(text: &str, font_size: u32, face: FontFace) -> f64 {
    let font = match face {
        Regular => REGULAR_FONT.get_or_init(|| FontRef::new(GEIST_REGULAR).ok()),
        SemiBold => SEMIBOLD_FONT.get_or_init(|| FontRef::new(GEIST_SEMIBOLD).ok()),
        Mono => MONO_FONT.get_or_init(|| FontRef::new(GEIST_MONO).ok()),
    };
    let Some(font) = font else {
        return text.chars().count() as f64 * f64::from(font_size);
    };
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::new(font_size as f32), LocationRef::default());
    text.chars()
        .map(|character| {
            let advance = charmap
                .map(character)
                .and_then(|glyph| metrics.advance_width(glyph));
            advance.map_or(f64::from(font_size), f64::from)
        })
        .sum()
}

fn card_width(diagram: &Diagram, node_index: usize, previous: Option<&str>) -> u32 {
    let node = &diagram.nodes[node_index];
    let label_width = node
        .label
        .iter()
        .map(String::as_str)
        .chain(previous)
        .map(|label| text_width(label, CARD_FONT_SIZE, Regular))
        .max()
        .unwrap_or_default();
    let right_inset = card_badge_label(node.mark).map_or(CARD_RIGHT_INSET, |label| {
        text_width(label, LABEL_FONT_SIZE, Mono).saturating_add(BADGE_RIGHT + CARD_BADGE_GAP)
    });
    CARD_WIDTH.max(
        label_width
            .saturating_add(CARD_TEXT_INSET)
            .saturating_add(right_inset),
    )
}

pub(super) fn card_badge_label(mark: Mark) -> Option<&'static str> {
    match mark {
        Mark::Added => Some("New"),
        Mark::Removed => Some("Removed"),
        Mark::Same | Mark::Changed => None,
    }
}

fn card_height(diagram: &Diagram, node_index: usize) -> u32 {
    let node = &diagram.nodes[node_index];
    let old_label_rows = u32::from(node.mark == Mark::Changed);
    let extra_rows = node.label.len().saturating_sub(1) as u32 + old_label_rows;
    CARD_HEIGHT + extra_rows * CARD_LINE_HEIGHT
}

struct Sections {
    first: Vec<Option<usize>>,
    neutral: bool,
}

impl Sections {
    fn new(diagram: &Diagram, forest: &Forest, links: &[Link<'_>]) -> Self {
        let mut first = vec![None; diagram.clusters.len()];
        let mut pending: Vec<_> = forest.roots.iter().rev().copied().collect();
        while let Some(node) = pending.pop() {
            if let Some(cluster) = diagram.node_cluster[node] {
                first[cluster].get_or_insert(node);
            }
            let branch = &forest.branches[node];
            pending.extend(branch.main.map(|index| links[index].to));
            pending.extend(branch.sides.iter().rev().map(|&index| links[index].to));
        }
        Self {
            first,
            neutral: diagram.clusters.is_empty(),
        }
    }

    fn jump_target(&self, diagram: &Diagram, source: usize, target: usize) -> String {
        let step = || diagram.nodes[target].label.join(" ");
        let Some(cluster) = diagram.node_cluster[target] else {
            return step();
        };
        if diagram.node_cluster[source] == Some(cluster) {
            return step();
        }
        let title = &diagram.clusters[cluster].title;
        if self.first[cluster] == Some(target) {
            return title.clone();
        }
        format!("{title} · {}", step())
    }
}

struct Link<'a> {
    from: usize,
    to: usize,
    label: Option<&'a str>,
    removed_overlay: bool,
}

fn edge_label_text(edge: &crate::model::Edge) -> Option<&str> {
    edge.label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
}

struct Graph<'a> {
    diagram: &'a Diagram,
    links: Vec<Link<'a>>,
    outgoing: Vec<Vec<usize>>,
    has_live_parent: Vec<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Active,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeKind {
    Tree,
    Back,
    Cross,
}

struct Forest {
    roots: Vec<usize>,
    branches: Vec<Branches>,
    jumps: Vec<Vec<(usize, JumpKind)>>,
}

#[derive(Default)]
struct Branches {
    main: Option<usize>,
    sides: Vec<usize>,
}

impl<'a> Graph<'a> {
    fn new(diagram: &'a Diagram) -> Self {
        let mut links = Vec::with_capacity(diagram.edges.len());
        let mut outgoing = vec![Vec::new(); diagram.nodes.len()];
        let mut has_live_parent = vec![false; diagram.nodes.len()];
        let has_live_nodes = diagram.nodes.iter().any(|node| node.mark != Mark::Removed);
        for (edge_index, edge) in diagram.edges.iter().enumerate() {
            let (from, to) = if edge.arrow_from && !edge.arrow_to {
                (edge.to, edge.from)
            } else {
                (edge.from, edge.to)
            };
            let removed_overlay = has_live_nodes && edge.mark == Mark::Removed;
            if !removed_overlay {
                has_live_parent[to] = true;
            }
            outgoing[from].push(edge_index);
            links.push(Link {
                from,
                to,
                label: edge_label_text(edge),
                removed_overlay,
            });
        }
        for exits in &mut outgoing {
            exits.sort_by_key(|&index| links[index].removed_overlay);
        }
        Self {
            diagram,
            links,
            outgoing,
            has_live_parent,
        }
    }

    fn walk(&self) -> Forest {
        let node_count = self.diagram.nodes.len();
        let mut visits = vec![Visit::New; node_count];
        let mut roots = Vec::new();
        let mut reserved_live_entries = vec![false; node_count];
        let mut preorder = Vec::with_capacity(node_count);
        let mut edge_kinds = vec![EdgeKind::Cross; self.links.len()];
        let actor = self
            .diagram
            .nodes
            .iter()
            .position(|node| node.shape == Shape::Stadium);
        let without_live_parent = (0..node_count).filter(|&node| !self.has_live_parent[node]);
        for root in actor
            .into_iter()
            .chain(without_live_parent)
            .chain(0..node_count)
        {
            if visits[root] != Visit::New {
                continue;
            }
            self.reserve_live_entries(root, &visits, &mut reserved_live_entries);
            roots.push(root);
            visits[root] = Visit::Active;
            preorder.push(root);
            let mut stack = vec![(root, 0)];
            while let Some((node, next)) = stack.last_mut() {
                let Some(&index) = self.outgoing[*node].get(*next) else {
                    visits[*node] = Visit::Done;
                    stack.pop();
                    continue;
                };
                *next += 1;
                let link = &self.links[index];
                let target = link.to;
                match visits[target] {
                    Visit::New if link.removed_overlay && reserved_live_entries[target] => {
                        edge_kinds[index] = EdgeKind::Cross;
                    }
                    Visit::New => {
                        if link.removed_overlay {
                            self.reserve_live_entries(target, &visits, &mut reserved_live_entries);
                        }
                        edge_kinds[index] = EdgeKind::Tree;
                        visits[target] = Visit::Active;
                        preorder.push(target);
                        stack.push((target, 0));
                    }
                    Visit::Active => edge_kinds[index] = EdgeKind::Back,
                    Visit::Done => {}
                }
            }
        }
        let mut branches = Vec::with_capacity(node_count);
        let mut jumps = Vec::with_capacity(node_count);
        for exits in &self.outgoing {
            let mut branch = Branches::default();
            let mut jump_edges = Vec::new();
            for &edge_index in exits {
                match edge_kinds[edge_index] {
                    EdgeKind::Tree => branch.sides.push(edge_index),
                    EdgeKind::Back => jump_edges.push((edge_index, JumpKind::Back)),
                    EdgeKind::Cross => jump_edges.push((edge_index, JumpKind::Go)),
                }
            }
            branches.push(branch);
            jumps.push(jump_edges);
        }
        let mut sizes = vec![1; node_count];
        for node in preorder.into_iter().rev() {
            let branch = &mut branches[node];
            let subtree_size = branch
                .sides
                .iter()
                .map(|&index| sizes[self.links[index].to])
                .sum();
            sizes[node] += subtree_size;
            branch.choose_main(
                &self.links,
                &sizes,
                subtree_size,
                &self.diagram.node_cluster,
            );
        }
        Forest {
            roots,
            branches,
            jumps,
        }
    }

    fn reserve_live_entries(&self, root: usize, visits: &[Visit], reserved: &mut [bool]) {
        // An old edge may attach a detached subtree without taking an entry from a live path.
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if visits[node] != Visit::New || reserved[node] {
                continue;
            }
            reserved[node] = true;
            pending.extend(
                self.outgoing[node]
                    .iter()
                    .map(|&index| &self.links[index])
                    .filter(|link| !link.removed_overlay)
                    .map(|link| link.to),
            );
        }
    }
}

impl Branches {
    fn choose_main(
        &mut self,
        links: &[Link<'_>],
        sizes: &[usize],
        total: usize,
        clusters: &[Option<usize>],
    ) {
        let dominant = self.sides.iter().position(|&index| {
            let size = sizes[links[index].to];
            size >= (total - size).saturating_mul(DOMINANT_BRANCH_RATIO)
        });
        if let Some(position) = dominant.or_else(|| self.single_section_exit(links, clusters)) {
            self.main = Some(self.sides.remove(position));
        }
    }

    fn single_section_exit(&self, links: &[Link<'_>], clusters: &[Option<usize>]) -> Option<usize> {
        let mut exits = self.sides.iter().enumerate().filter(|&(_, &index)| {
            let link = &links[index];
            clusters[link.from] != clusters[link.to]
        });
        let (position, _) = exits.next()?;
        exits.next().is_none().then_some(position)
    }
}

#[derive(Clone, Copy)]
struct Run {
    cluster: Option<usize>,
    band: usize,
}

struct PlacedSides {
    last_row: u32,
    bottom: u32,
    lowest_band_bottom: u32,
}

struct Placer<'a> {
    graph: &'a Graph<'a>,
    forest: &'a Forest,
    sections: &'a Sections,
    previous_labels: &'a [Option<&'a str>],
    image: Layout,
    titled: Vec<bool>,
    neutral_band: Option<usize>,
    marked: bool,
}

impl Placer<'_> {
    fn place(mut self) -> Layout {
        let left = CANVAS_INSET + SECTION_LEFT_INSET;
        let mut top = self.image.height;
        for &root in &self.forest.roots {
            top += SECTION_HEADING_HEIGHT;
            let (_, bottom) = self.place_chain(root, left, top, None, None);
            self.image.height = bottom + SECTION_INSET + CANVAS_INSET;
            top = bottom + SECTION_INSET + SECTION_GAP;
        }
        self.image
            .connections
            .sort_by_key(|connection| connection.edge_index);
        self.image
    }

    fn cluster(&self, node: usize) -> Option<usize> {
        self.graph.diagram.node_cluster[node]
    }

    fn place_chain(
        &mut self,
        mut node: usize,
        left: u32,
        mut top: u32,
        mut run: Option<Run>,
        parent: Option<usize>,
    ) -> (Rect, u32) {
        let mut first = None;
        loop {
            let current = self.continue_run(node, left, top, run, parent);
            let bounds = self.place_card(node, left, top, current.band);
            first.get_or_insert(bounds);
            let jump_bottom = self.place_jumps(node, bounds, current.band);
            let sides = self.place_sides(node, bounds, jump_bottom, current);
            self.extend_band(current, sides.bottom);
            let Some(edge_index) = self.forest.branches[node].main else {
                return (first.unwrap_or(bounds), sides.bottom);
            };
            top = self.place_continuation(edge_index, bounds, sides, current);
            node = self.graph.links[edge_index].to;
            run = Some(current);
        }
    }

    fn place_sides(&mut self, node: usize, bounds: Rect, mut bottom: u32, run: Run) -> PlacedSides {
        let sides = &self.forest.branches[node].sides;
        let spine = bounds.x.saturating_add(SPINE_INSET);
        let mut last_row = bounds.bottom();
        let mut lowest_band_bottom = 0u32;
        let indent = self.branch_indent(sides);
        for &edge_index in sides {
            let link = &self.graph.links[edge_index];
            let cluster = self.cluster(link.to);
            let top = self
                .next_top(bottom, run.cluster, cluster, false)
                .max(lowest_band_bottom.saturating_add(SECTION_GAP));
            let first_new_band = self.image.sections.len();
            let (child, block_bottom) = self.place_chain(
                link.to,
                bounds.x.saturating_add(indent),
                top,
                Some(run),
                Some(run.band),
            );
            if let Some(block_band_bottom) = self.lowest_band_bottom_since(first_new_band) {
                lowest_band_bottom = lowest_band_bottom.max(block_band_bottom);
            }
            let middle = child.y + child.height / 2;
            let start = Point {
                x: spine,
                y: middle,
            };
            let end = Point {
                x: child.x,
                y: middle,
            };
            let arrow_space = self.stub_arrow_space(link);
            let label = link
                .label
                .map(|text| centered_label(text, start, end, arrow_space));
            self.connect(
                edge_index,
                label,
                Route::Line {
                    points: vec![start, end],
                },
                run.band,
            );
            last_row = middle;
            bottom = block_bottom;
        }
        if !sides.is_empty() {
            self.image.buses.push(Bus {
                from: node,
                points: [
                    Point {
                        x: spine,
                        y: bounds.bottom(),
                    },
                    Point {
                        x: spine,
                        y: last_row,
                    },
                ],
            });
        }
        PlacedSides {
            last_row,
            bottom,
            lowest_band_bottom,
        }
    }

    fn lowest_band_bottom_since(&self, first_band: usize) -> Option<u32> {
        self.image.sections[first_band..]
            .iter()
            .map(|band| band.bounds.bottom())
            .max()
    }

    fn next_top(
        &self,
        bottom: u32,
        previous: Option<usize>,
        next: Option<usize>,
        labelled: bool,
    ) -> u32 {
        let gap = if labelled {
            LABELLED_CARD_GAP
        } else {
            CARD_GAP
        };
        let band_gap = if previous != next {
            SECTION_INSET + SECTION_GAP + SECTION_HEADING_HEIGHT
        } else {
            0
        };
        bottom + gap.max(band_gap)
    }

    fn place_continuation(
        &mut self,
        edge_index: usize,
        card: Rect,
        sides: PlacedSides,
        run: Run,
    ) -> u32 {
        let PlacedSides {
            last_row,
            bottom,
            lowest_band_bottom,
        } = sides;
        let link = &self.graph.links[edge_index];
        let next = self.cluster(link.to);
        let enters_band = run.cluster != next;
        let label_start = bottom.max(lowest_band_bottom);
        let mut continuation_bottom = bottom;
        if enters_band && link.label.is_some() {
            continuation_bottom = label_start.saturating_add(LABEL_CLEARANCE + EDGE_LABEL_HEIGHT);
            self.extend_band(run, continuation_bottom);
        }
        let top = self
            .next_top(continuation_bottom, run.cluster, next, link.label.is_some())
            .max(lowest_band_bottom.saturating_add(SECTION_GAP));
        let spine = card.x.saturating_add(SPINE_INSET);
        let points = vec![
            Point {
                x: spine,
                y: last_row,
            },
            Point { x: spine, y: top },
        ];
        let label = link.label.map(|text| {
            let left = spine.saturating_add(VERTICAL_LABEL_INSET - EDGE_LABEL_INSET);
            let center = if enters_band {
                label_start.saturating_add(LABEL_CLEARANCE + EDGE_LABEL_HEIGHT / 2)
            } else {
                label_start.saturating_add((top - label_start) / 2)
            };
            edge_label(text, left, center)
        });
        self.connect(edge_index, label, Route::Line { points }, run.band);
        top
    }

    fn continue_run(
        &mut self,
        node: usize,
        left: u32,
        top: u32,
        run: Option<Run>,
        parent: Option<usize>,
    ) -> Run {
        let cluster = self.cluster(node);
        if let Some(run) = run.filter(|run| run.cluster == cluster) {
            return run;
        }
        if self.sections.neutral {
            let band = match self.neutral_band {
                Some(band) => band,
                None => {
                    let band = self.start_band(None, left, top, None, None);
                    self.neutral_band = Some(band);
                    band
                }
            };
            return Run { cluster, band };
        }
        let heading = cluster.and_then(|cluster_index| {
            if std::mem::replace(&mut self.titled[cluster_index], true) {
                return None;
            }
            Some(Point {
                x: left.saturating_add(CARD_TEXT_INSET),
                y: top - SECTION_HEADING_HEIGHT + SECTION_HEADING_BASELINE,
            })
        });
        Run {
            cluster,
            band: self.start_band(cluster, left, top, heading, parent),
        }
    }

    fn start_band(
        &mut self,
        cluster_index: Option<usize>,
        left: u32,
        top: u32,
        heading: Option<Point>,
        parent: Option<usize>,
    ) -> usize {
        let depth = parent.map_or(0, |index| self.image.sections[index].depth + 1);
        if let (Some(index), Some(origin)) = (cluster_index, heading) {
            let cluster = &self.graph.diagram.clusters[index];
            let status_width = if self.marked {
                text_width(SECTION_STATUS_LABEL, LABEL_FONT_SIZE, Mono)
                    .saturating_add(SECTION_STATUS_INSET + SECTION_STATUS_GAP)
            } else {
                0
            };
            self.include_width(
                origin
                    .x
                    .saturating_add(text_width(&cluster.title, SECTION_FONT_SIZE, SemiBold))
                    .saturating_add(status_width),
                depth,
            );
        }
        let band = self.image.sections.len();
        self.image.sections.push(Section {
            cluster_index,
            bounds: Rect {
                x: left - SECTION_LEFT_INSET,
                y: top - SECTION_HEADING_HEIGHT,
                width: 0,
                height: 0,
            },
            heading,
            parent,
            depth,
        });
        band
    }

    fn extend_band(&mut self, run: Run, bottom: u32) {
        let bounds = &mut self.image.sections[run.band].bounds;
        bounds.height = bounds.height.max(bottom + SECTION_INSET - bounds.y);
    }

    fn place_card(&mut self, node: usize, left: u32, top: u32, band: usize) -> Rect {
        let diagram = self.graph.diagram;
        let previous = self.previous_labels.get(node).copied().flatten();
        let bounds = Rect {
            x: left,
            y: top,
            width: card_width(diagram, node, previous),
            height: card_height(diagram, node),
        };
        self.include_width(bounds.right(), self.image.sections[band].depth);
        self.image.cards.push(Card {
            node_index: node,
            band_index: band,
            bounds,
        });
        bounds
    }

    fn place_jumps(&mut self, node: usize, card: Rect, band: usize) -> u32 {
        let mut bottom = card.bottom();
        let mut row = card.y + card.height / 2;
        let edges = &self.forest.jumps[node];
        let elbow = if edges.len() > 1 { JUMP_ELBOW } else { 0 };
        let right = card.right();
        let stem_start = Point { x: right, y: row };
        let mut rows = Vec::new();
        for &(edge_index, kind) in edges {
            let start =
                self.place_jump_row(edge_index, kind, Point { x: right, y: row }, elbow, band);
            if elbow > 0 {
                rows.push(JumpRow {
                    edge_index,
                    point: start,
                });
            }
            bottom = bottom.max(row + EDGE_LABEL_HEIGHT / 2);
            row += EDGE_ROW_HEIGHT;
        }
        if !rows.is_empty() {
            self.image.jump_stems.push(JumpStem {
                from: node,
                start: stem_start,
                rows,
            });
        }
        bottom
    }

    fn place_jump_row(
        &mut self,
        edge_index: usize,
        kind: JumpKind,
        card_edge: Point,
        elbow: u32,
        band: usize,
    ) -> Point {
        let link = &self.graph.links[edge_index];
        let start = Point {
            x: card_edge.x.saturating_add(elbow),
            y: card_edge.y,
        };
        let end = Point {
            x: card_edge.x.saturating_add(jump_length(link.label, elbow)),
            y: card_edge.y,
        };
        let target = self
            .sections
            .jump_target(self.graph.diagram, link.from, link.to);
        let origin = Point {
            x: end.x.saturating_add(JUMP_CONTENT_GAP),
            y: card_edge.y + LABEL_BASELINE_OFFSET,
        };
        let label = link.label.map(|text| centered_label(text, start, end, 0));
        self.include_width(
            jump_bounds(origin, kind, &target).right(),
            self.image.sections[band].depth,
        );
        let route = Route::Jump {
            points: vec![start, end],
            origin,
            kind,
            target,
        };
        self.connect(edge_index, label, route, band);
        start
    }

    fn connect(&mut self, edge_index: usize, label: Option<EdgeLabel>, route: Route, band: usize) {
        let link = &self.graph.links[edge_index];
        if let Some(label) = &label {
            self.include_width(label.bounds.right(), self.image.sections[band].depth);
        }
        self.image.connections.push(Connection {
            edge_index,
            from: link.from,
            to: link.to,
            label,
            route,
        });
    }

    fn include_width(&mut self, right: u32, depth: u32) {
        let inset = CANVAS_INSET + (depth + 1) * SECTION_INSET;
        self.image.width = self.image.width.max(right.saturating_add(inset));
    }

    fn branch_indent(&self, sides: &[usize]) -> u32 {
        let mut indent = BRANCH_INDENT;
        for &index in sides {
            let link = &self.graph.links[index];
            if let Some(label) = link.label {
                let space = SPINE_INSET + STUB_LABEL_PADDING + self.stub_arrow_space(link);
                indent =
                    indent.max(text_width(label, LABEL_FONT_SIZE, SemiBold).saturating_add(space));
            }
        }
        indent
    }

    fn stub_arrow_space(&self, link: &Link<'_>) -> u32 {
        if self.cluster(link.from) == self.cluster(link.to) {
            STUB_ARROW_SPACE
        } else {
            SECTION_LEFT_INSET + LABEL_CLEARANCE
        }
    }
}

impl JumpKind {
    pub(super) fn prefix(self) -> &'static str {
        match self {
            Self::Back => "Back to ",
            Self::Go => "Go to ",
        }
    }
}

fn jump_length(label: Option<&str>, elbow: u32) -> u32 {
    label.map_or(JUMP_LINE_LENGTH, |text| {
        let padded =
            text_width(text, LABEL_FONT_SIZE, SemiBold).saturating_add(JUMP_LABEL_PADDING + elbow);
        JUMP_LABEL_LINE_LENGTH.max(padded)
    })
}

fn centered_label(text: &str, start: Point, end: Point, arrow_space: u32) -> EdgeLabel {
    let available = end.x.saturating_sub(start.x).saturating_sub(arrow_space);
    let padding = available.saturating_sub(edge_label_width(text)) / 2;
    edge_label(text, start.x.saturating_add(padding), start.y)
}

fn edge_label_width(text: &str) -> u32 {
    text_width(text, LABEL_FONT_SIZE, SemiBold).saturating_add(2 * EDGE_LABEL_INSET)
}

fn edge_label(text: &str, left: u32, center_y: u32) -> EdgeLabel {
    let width = edge_label_width(text);
    EdgeLabel {
        text: text.to_owned(),
        origin: Point {
            x: left.saturating_add(width / 2),
            y: center_y + LABEL_BASELINE_OFFSET,
        },
        bounds: Rect {
            x: left,
            y: center_y - EDGE_LABEL_HEIGHT / 2,
            width,
            height: EDGE_LABEL_HEIGHT,
        },
    }
}

fn jump_bounds(origin: Point, kind: JumpKind, target: &str) -> Rect {
    Rect {
        x: origin.x,
        y: origin.y - LABEL_BASELINE_OFFSET - EDGE_LABEL_HEIGHT / 2,
        width: text_width(kind.prefix(), LABEL_FONT_SIZE, SemiBold)
            .saturating_add(text_width(target, LABEL_FONT_SIZE, SemiBold))
            .saturating_add(JUMP_TEXT_INSET),
        height: EDGE_LABEL_HEIGHT,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::diff;
    use crate::flow_image::{ImageOptions, Theme, parse_flow, prepare_image, svg};
    use crate::flows::{self, Flow};
    use crate::parser;

    const FLOW_CATALOGS: &[&str] = &[
        "docs/flows",
        "tests/fixtures/corpus/commerce",
        "tests/fixtures/corpus/healthchecks",
        "tests/fixtures/corpus/shiori",
        "tests/fixtures/flows/after",
        "tests/fixtures/flows/before",
        "tests/fixtures/flows/browse",
    ];
    const MAX_CATALOG_WIDTH: u32 = 1278;
    const NESTED_SECTION_FORK: &str = "subgraph source [Source]
choice{Choice?} -->|yes| prepare[Prepare]
choice --> skip[Skip]
skip --> first[First]
end
subgraph nested [Nested]
result[Set the result]
end
prepare --> result";
    const DOMINANT_CONTINUATION_STEPS: &str = "\nfirst --> second[Second] --> third[Third]";

    #[test]
    fn stub_labels_keep_a_visible_line() {
        let diagram = parser::parse(
            "flowchart TD
choice{View?} -->|change comparison| compare[Compare changes]
choice -->|no| skip[Skip]",
        )
        .unwrap();
        let image = layout(&diagram, "Flow", None);
        let connection = &image.connections[0];
        let Route::Line { points } = &connection.route else {
            panic!("the branch must have a stub");
        };
        let label = connection.label.as_ref().unwrap().bounds;
        let bus = &image.buses[0];
        let child = node_card(&image, &diagram, "compare");
        assert_eq!(points[0].x, bus.points[0].x);
        assert_eq!(points[0].y, points[1].y);
        assert_eq!(points[1].x, child.bounds.x);
        const MINIMUM_VISIBLE_LINE: u32 = 20;
        assert!(label.x >= bus.points[0].x + MINIMUM_VISIBLE_LINE);
        assert!(label.right() + MINIMUM_VISIBLE_LINE <= child.bounds.x - STUB_ARROW_SPACE);
    }

    #[test]
    fn labelled_continuation_is_centered_in_a_52_pixel_gap() {
        let diagram = parser::parse("flowchart TD\nbefore[Before] -->|next| after[After]").unwrap();
        let image = layout(&diagram, "Flow", None);
        let from = node_card(&image, &diagram, "before");
        let to = node_card(&image, &diagram, "after");
        let label = image.connections[0].label.as_ref().unwrap().bounds;
        let gap = to.bounds.y - from.bounds.bottom();
        assert_eq!(gap, 52);
        assert_eq!(label.y + label.height / 2, from.bounds.bottom() + gap / 2);
    }

    #[test]
    fn side_after_a_nested_band_keeps_the_section_gap() {
        for extra_steps in ["", DOMINANT_CONTINUATION_STEPS] {
            let diagram =
                parser::parse(&format!("flowchart TD\n{NESTED_SECTION_FORK}{extra_steps}"))
                    .unwrap();
            let image = layout(&diagram, "Flow", None);
            let choice = node_card(&image, &diagram, "choice");
            let result = node_card(&image, &diagram, "result");
            let next = node_card(&image, &diagram, "skip");
            let nested_band = &image.sections[result.band_index];
            assert_eq!(nested_band.parent, Some(choice.band_index));
            assert_eq!(next.bounds.x == choice.bounds.x, !extra_steps.is_empty());
            assert!(
                next.bounds.y >= nested_band.bounds.bottom() + SECTION_GAP,
                "next card at {}, nested band ends at {}",
                next.bounds.y,
                nested_band.bounds.bottom()
            );
        }
    }

    #[test]
    fn long_continuation_label_clears_the_nested_side_band() {
        let source = NESTED_SECTION_FORK.replace(
            "choice --> skip",
            "choice -->|A deliberately long continuation label| skip",
        );
        let flow = flows::parse_flow(
            "nested-continuation.md",
            &format!(
                "# Continue\n\n```mermaid\nflowchart TD\n{source}{DOMINANT_CONTINUATION_STEPS}\n```"
            ),
        );
        let diagram = parse_flow(&flow).unwrap();
        assert_clean_layout(&flow, &diagram, ImageOptions::default());
    }

    #[test]
    fn nested_branches_indent_and_merge_at_one_step() {
        let diagram = parser::parse(
            "flowchart TD
actor([Start]) --> question{Choice?}
question -->|yes| follow{Continue?}
follow -->|yes| approve[Approve]
follow -->|no| retry[Try again]
approve --> finish[Finish]
retry --> finish
question -->|no| cancel[Cancel]
cancel --> finish",
        )
        .unwrap();
        let image = layout(&diagram, "A flow", None);
        let decision = node_card(&image, &diagram, "question").bounds;
        let nested_decision = node_card(&image, &diagram, "follow").bounds;
        let nested_branch = node_card(&image, &diagram, "approve").bounds;
        let sibling_branch = node_card(&image, &diagram, "cancel").bounds;
        assert_eq!(nested_decision.x, decision.x);
        assert_eq!(sibling_branch.x, decision.x + BRANCH_INDENT);
        assert_eq!(nested_branch.x, nested_decision.x);
        assert!(sibling_branch.y < nested_decision.y);
        let finish = node_card(&image, &diagram, "finish").node_index;
        let arrivals: Vec<_> = image
            .connections
            .iter()
            .filter(|connection| connection.to == finish)
            .map(|connection| &connection.route)
            .collect();
        assert!(
            matches!(arrivals.as_slice(), [Route::Line { .. }, Route::Jump { kind: JumpKind::Go, target: first, .. }, Route::Jump { kind: JumpKind::Go, target: second, .. }] if first == "Finish" && second == "Finish")
        );
    }

    #[test]
    fn descriptions_use_unrounded_advances_and_the_actual_content_width() {
        let short = "note ".repeat(23);
        let narrow = parser::parse("flowchart TD\nstart[Start]").unwrap();
        assert_eq!(layout(&narrow, "Flow", Some(&short)).description.len(), 1);

        let long = "note ".repeat(27);
        assert!(layout(&narrow, "Flow", Some(&long)).description.len() > 1);
        let wide_step = "W".repeat(90);
        let wide = parser::parse(&format!("flowchart TD\nstart[{wide_step}]")).unwrap();
        assert_eq!(layout(&wide, "Flow", Some(&long)).description.len(), 1);
    }

    #[test]
    fn marked_sections_leave_space_beside_a_long_heading() {
        let title = "Wide section heading for a detailed review of the proposed changes";
        let diagram = parser::parse(&format!(
            "flowchart TD\nsubgraph review [{title}]\nactor([Start]) --> finish[Finish]\nend"
        ))
        .unwrap();
        let difference = diff::all_added(&diagram);
        let image = layout(&difference.union, "Flow", None);
        let section = &image.sections[0];
        let heading_right =
            section.heading.unwrap().x + text_width(title, SECTION_FONT_SIZE, SemiBold);
        let status_width = text_width(SECTION_STATUS_LABEL, LABEL_FONT_SIZE, Mono);
        assert!(heading_right + status_width + SECTION_STATUS_INSET <= section.bounds.right());
    }

    #[test]
    fn names_back_and_cross_targets_by_first_card_in_the_section() {
        let diagram = parser::parse(
            "flowchart TD
subgraph cart [Cart]
    actor([Start]) --> edit[Edit cart]
    edit --> checkout{Ready?}
end
subgraph pay [Payment]
    payment[Enter card] --> submit[Submit]
    submit -->|retry| payment
end
subgraph done [Summary]
    receipt[See receipt]
end
checkout -->|yes| payment
checkout -->|skip payment| receipt
submit -->|change cart| actor
submit -->|wrong items| edit
submit -->|paid| receipt",
        )
        .unwrap();
        let image = layout(&diagram, "Checkout", None);
        assert_eq!(image.sections.len(), 3);
        for (label, expected_kind, expected_target) in [
            ("skip payment", JumpKind::Go, "Summary"),
            ("change cart", JumpKind::Back, "Cart"),
            ("wrong items", JumpKind::Back, "Cart · Edit cart"),
            ("retry", JumpKind::Back, "Enter card"),
        ] {
            let connection = image
                .connections
                .iter()
                .find(|connection| {
                    connection
                        .label
                        .as_ref()
                        .is_some_and(|edge_label| edge_label.text == label)
                })
                .unwrap();
            let Route::Jump {
                kind,
                target,
                points,
                ..
            } = &connection.route
            else {
                panic!("{label} must be a jump");
            };
            assert_eq!((*kind, target.as_str()), (expected_kind, expected_target));
            assert_eq!(points.len(), 2);
        }
    }

    #[test]
    fn sectionless_flow_keeps_one_neutral_section_and_names_the_loop_step() {
        let diagram =
            parser::parse("flowchart TD\nstart([Start]) --> step[Edit]\nstep --> step").unwrap();
        let image = layout(&diagram, "Editing", None);
        assert_eq!(image.sections.len(), 1);
        assert_eq!(image.sections[0].cluster_index, None);
        assert_eq!(image.sections[0].heading, None);
        let Route::Jump { kind, target, .. } = &image.connections[1].route else {
            panic!("a self-loop must use a jump");
        };
        assert_eq!((*kind, target.as_str()), (JumpKind::Back, "Edit"));
    }

    #[test]
    fn removed_link_does_not_turn_a_live_path_into_a_back_jump() {
        let before = parser::parse("flowchart TD\na([Start]) --> b[B]\nb --> c[C]").unwrap();
        let after =
            parser::parse("flowchart TD\na([Start]) --> b[B]\na --> c[C]\nc --> b").unwrap();
        let union = diff::diff(&before, &after).union;
        let image = layout(&union, "Changed flow", None);
        let live_link = image
            .connections
            .iter()
            .find(|connection| {
                union.nodes[connection.from].id == "c" && union.nodes[connection.to].id == "b"
            })
            .unwrap();
        assert!(
            matches!(
                live_link.route,
                Route::Jump {
                    kind: JumpKind::Go,
                    ..
                }
            ),
            "{live_link:?}"
        );
        assert!(node_card(&image, &union, "b").bounds.y < node_card(&image, &union, "c").bounds.y);
    }

    #[test]
    fn removed_bridge_attaches_a_disconnected_live_component() {
        let before =
            parser::parse("flowchart TD\na([Actor]) --> c[C]\nc --> d[D]\nd --> c").unwrap();
        let after = parser::parse("flowchart TD\na([Actor])\nc[C] --> d[D]\nd --> c").unwrap();
        let union = diff::diff(&before, &after).union;
        let image = layout(&union, "Changed flow", None);
        let bridge = image
            .connections
            .iter()
            .find(|connection| union.nodes[connection.from].id == "a")
            .unwrap();
        assert_eq!(union.edges[bridge.edge_index].mark, Mark::Removed);
        assert!(matches!(bridge.route, Route::Line { .. }));
    }

    #[test]
    fn steps_outside_named_sections_keep_a_neutral_band() {
        let diagram = parser::parse(
            "flowchart TD\na([Actor])\nsubgraph named [Named]\nb[Work]\nend\na --> b",
        )
        .unwrap();
        let image = layout(&diagram, "Mixed flow", None);
        let actor = node_card(&image, &diagram, "a");
        let band = &image.sections[actor.band_index];
        assert_eq!(band.cluster_index, None);
        assert_eq!(band.heading, None);
        assert!(band.bounds.bottom() >= actor.bounds.bottom());
    }

    #[test]
    fn selecting_a_later_jump_keeps_the_shared_stem_visible() {
        let flow = flows::parse_flow(
            "fork.md",
            "# Fork\n\n```mermaid\nflowchart TD\nactor([Actor]) --> unrelated[Unrelated]\nactor --> target[Target]\nactor --> router[Router]\nrouter --> unrelated\nrouter --> target\n```\n",
        );
        let diagram = parse_flow(&flow).unwrap();
        let layout = layout(&diagram, &flow.title, None);
        let selected = node_card(&layout, &diagram, "target").node_index;
        let options = ImageOptions {
            selected_step: Some(selected),
            ..ImageOptions::default()
        };
        let rendered = svg(&flow, &diagram, &Theme::dark(), options);
        let document = resvg::usvg::roxmltree::Document::parse(&rendered).unwrap();
        let stem = &layout.jump_stems[0];
        assert_eq!(stem.from, node_card(&layout, &diagram, "router").node_index);
        for [start, end] in stem.segments() {
            let points = format!("{},{} {},{}", start.x, start.y, end.x, end.y);
            let line = document
                .descendants()
                .find(|node| node.attribute("points") == Some(&points))
                .expect("the shared jump stem is drawn");
            assert!(
                !line
                    .ancestors()
                    .any(|node| node.attribute("opacity") == Some(DIMMED_OPACITY)),
                "the shared stem is visible even when the first jump is dimmed"
            );
        }
    }

    #[test]
    fn comparison_reserves_space_for_a_long_base_caption() {
        let flow = flows::parse_flow("flow.md", "# Flow");
        let diagram = parser::parse("flowchart TD\nactor([Actor])").unwrap();
        let base = "W".repeat(100);
        let caption = format!("Before · {base}");
        let expected_minimum =
            text_width(&caption, COMPARISON_CAPTION_SIZE, SemiBold) + 2 * COMPARISON_CAPTION.x;
        let rendered = crate::flow_image::side_by_side_svg(
            Some((&flow, &diagram)),
            None,
            &Theme::dark(),
            &base,
        );
        let document = resvg::usvg::roxmltree::Document::parse(&rendered).unwrap();
        let divider = document.root_element().last_element_child().unwrap();
        let path = divider.attribute("d").unwrap();
        let left_width: u32 = path.split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!(left_width >= expected_minimum);
    }

    #[test]
    fn every_flow_draws_cleanly() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut checked = 0;
        for catalog in FLOW_CATALOGS {
            for flow in flows::load_dir(&root.join(catalog)).unwrap() {
                if let Ok(diagram) = parse_flow(&flow) {
                    assert_clean_layout(&flow, &diagram, ImageOptions::default());
                    checked += 1;
                }
            }
        }
        assert!(checked >= 40);

        let checkout = |side| {
            let file = root.join(format!("tests/fixtures/flows/{side}/checkout.md"));
            flows::parse_flow("checkout.md", &std::fs::read_to_string(file).unwrap())
        };
        let before = checkout("before");
        let after = checkout("after");
        let difference = diff::diff(&parse_flow(&before).unwrap(), &parse_flow(&after).unwrap());
        let options = ImageOptions {
            changes: Some(&difference.changes),
            selected_step: None,
        };
        assert_clean_layout(&after, &difference.union, options);
        let prepared = prepare_image(&after, &difference.union, options.changes, false);
        let address = node_card(&prepared.layout, &prepared.diagram, "address");
        let cart = node_card(&prepared.layout, &prepared.diagram, "cart");
        assert_eq!(address.bounds.x, cart.bounds.x + BRANCH_INDENT);
        assert_eq!(
            prepared.diagram.nodes[address.node_index].mark,
            Mark::Removed
        );
        assert!(prepared.layout.connections.iter().any(|connection| {
            connection.from == cart.node_index
                && connection.to == address.node_index
                && matches!(connection.route, Route::Line { .. })
        }));
    }

    #[test]
    fn edge_case_flows_draw_cleanly() {
        let cases = [
            ("self-loop", "a([Start]) -->|again| a\na -->|done| b[End]"),
            (
                "all exits are jumps",
                "a([Start]) --> b[Work] --> c{Retry?}\nc -->|restart| a\nc -->|repeat| b",
            ),
            (
                "multiple jumps",
                "a([Start]) --> b[First] --> c[Second] --> d[Last]\nd -->|start| a\nd -->|first| b\nd -->|second| c",
            ),
            (
                "fork with a jump",
                "a([Start]) --> b{Ready?}\nb -->|error| c[Error]\nb -->|yes| d[Continue] --> e[Finish]\nb -->|retry| a",
            ),
            (
                "section change without dominance",
                "subgraph source [Source]\na{Ready?} -->|no| b[Try again]\nend\nsubgraph next [Next]\nc[Finish]\nend\na -->|yes| c",
            ),
            (
                "long side label",
                "subgraph source [Source]\na{Ready?} -->|yes| b[First] --> c[Second]\nend\nsubgraph side [Side]\nd[Error]\nend\na -->|some other outcome| d",
            ),
            ("nested section", NESTED_SECTION_FORK),
            (
                "two branches into one foreign section",
                "subgraph source [Source]\na{Choice?}\nend\nsubgraph side [Side]\nb[First]\nc[Second]\nend\na -->|first| b\na -->|second| c",
            ),
            (
                "empty section",
                "subgraph s_x [X]\nend\na([Start]) --> b[End]",
            ),
            (
                "join of three",
                "a{Choose} -->|one| b[First]\na -->|two| c[Second]\na -->|three| d[Third]\nb --> e[Join]\nc --> e\nd --> e",
            ),
        ];
        for (name, source) in cases {
            let flow = flows::parse_flow(
                &format!("{name}.md"),
                &format!("# {name}\n\n```mermaid\nflowchart TD\n{source}\n```"),
            );
            let diagram = parse_flow(&flow).unwrap();
            assert_clean_layout(&flow, &diagram, ImageOptions::default());

            let image = layout(&diagram, &flow.title, None);
            if name == "section change without dominance" {
                let source = node_card(&image, &diagram, "a");
                let continuation = node_card(&image, &diagram, "c");
                let side = node_card(&image, &diagram, "b");
                assert_eq!(source.bounds.x, continuation.bounds.x, "{name}");
                assert_continuation_clearance(&image, source, continuation, side.bounds.bottom());
            }
            if name == "empty section" {
                assert_eq!(image.sections.len(), 1);
                assert_eq!(image.sections[0].cluster_index, None);
                assert_eq!(image.sections[0].heading, None);
            }
        }

        let flow = flows::parse_flow("removed.md", "# Removed");
        let before = parser::parse("flowchart TD\na([Start]) --> b[End]\nb --> a").unwrap();
        let empty = Diagram {
            direction: before.direction,
            nodes: Vec::new(),
            edges: Vec::new(),
            clusters: Vec::new(),
            node_cluster: Vec::new(),
        };
        let difference = diff::diff(&before, &empty);
        assert_clean_layout(
            &flow,
            &difference.union,
            ImageOptions {
                changes: Some(&difference.changes),
                selected_step: None,
            },
        );
    }

    fn assert_continuation_clearance(
        image: &Layout,
        source: &Card,
        target: &Card,
        side_bottom: u32,
    ) {
        let label = image
            .connections
            .iter()
            .find_map(|edge| {
                if edge.from == source.node_index && edge.to == target.node_index {
                    return edge.label.as_ref().map(|label| label.bounds);
                }
                None
            })
            .unwrap();
        let source_band = &image.sections[source.band_index];
        let target_band = &image.sections[target.band_index];
        const MINIMUM_CLEARANCE: u32 = 8;
        assert!(label.y >= source.bounds.bottom().max(side_bottom) + MINIMUM_CLEARANCE);
        assert!(label.bottom() + MINIMUM_CLEARANCE <= source_band.bounds.bottom());
        assert!(label.bottom() + MINIMUM_CLEARANCE <= target_band.bounds.y);
        assert!(
            label.bottom() + MINIMUM_CLEARANCE
                <= target_band.heading.unwrap().y - SECTION_FONT_SIZE
        );
    }

    fn assert_clean_layout(flow: &Flow, diagram: &Diagram, options: ImageOptions<'_>) {
        let path = &flow.path;
        let prepared = prepare_image(flow, diagram, options.changes, false);
        let layout = &prepared.layout;
        assert_eq!(layout.cards.len(), diagram.nodes.len(), "{path}");
        assert_eq!(layout.connections.len(), diagram.edges.len(), "{path}");
        assert!(
            layout.width <= MAX_CATALOG_WIDTH,
            "{path} is {} px wide",
            layout.width
        );
        let mut cards_by_node: Vec<_> = layout.cards.iter().collect();
        cards_by_node.sort_unstable_by_key(|card| card.node_index);
        assert_cards_in_bands(path, &prepared.diagram, layout, &cards_by_node);
        let (routes, labels) = assert_routes(path, layout, &cards_by_node);
        let mut segments = routes;
        let labelled_edges = layout
            .connections
            .iter()
            .filter(|connection| connection.label.is_some())
            .count();
        assert_eq!(
            labelled_edges,
            prepared
                .diagram
                .edges
                .iter()
                .filter(|edge| edge_label_text(edge).is_some())
                .count(),
            "{path}"
        );
        segments.extend(layout.buses.iter().map(|bus| Segment {
            start: bus.points[0],
            end: bus.points[1],
            from: bus.from,
            edge_index: None,
        }));
        for stem in &layout.jump_stems {
            segments.extend(stem.segments().map(|[start, end]| Segment {
                start,
                end,
                from: stem.from,
                edge_index: None,
            }));
        }
        assert_segments_clear(path, &segments, &layout.cards);
        assert_labels_clear(path, &labels, &segments, layout, &cards_by_node);
        assert_headings_clear(path, &prepared.diagram, layout, &labels, &segments);
        assert_eq!(
            *layout,
            prepare_image(flow, diagram, options.changes, false).layout,
            "{path}"
        );
        let rendered = svg(flow, diagram, &Theme::dark(), options);
        assert_eq!(
            rendered,
            svg(flow, diagram, &Theme::dark(), options),
            "{path}"
        );
        assert_eq!(
            rendered.matches("data-edge-label=").count(),
            labelled_edges,
            "{path}"
        );
    }

    fn assert_cards_in_bands(path: &str, diagram: &Diagram, layout: &Layout, cards: &[&Card]) {
        for (index, card) in cards.iter().enumerate() {
            assert_eq!(index, card.node_index, "{path}");
            let band = &layout.sections[card.band_index];
            assert_eq!(band.cluster_index, diagram.node_cluster[index], "{path}");
            assert!(
                encloses(band.bounds, card.bounds),
                "{path}: {card:?} is outside {band:?}"
            );
            for other in &cards[index + 1..] {
                assert!(
                    !overlaps(card.bounds, other.bounds),
                    "{path}: {card:?} overlaps {other:?}"
                );
            }
        }
        for (index, band) in layout.sections.iter().enumerate() {
            for other in &layout.sections[index + 1..] {
                if band.parent == other.parent {
                    assert!(
                        !overlaps(band.bounds, other.bounds),
                        "{path}: sibling bands overlap: {band:?}, {other:?}"
                    );
                }
            }
            if let Some(parent) = band.parent {
                let parent = layout.sections[parent].bounds;
                assert!(
                    encloses(parent, band.bounds),
                    "{path}: {band:?} is outside {parent:?}"
                );
                assert_eq!(
                    band.bounds.right() + SECTION_INSET,
                    parent.right(),
                    "{path}"
                );
            }
        }
    }

    fn assert_routes(
        path: &str,
        layout: &Layout,
        cards: &[&Card],
    ) -> (Vec<Segment>, Vec<(usize, Rect)>) {
        let mut parents = vec![None; cards.len()];
        let mut segments = Vec::new();
        let mut labels = Vec::new();
        for (index, connection) in layout.connections.iter().enumerate() {
            assert_eq!(connection.edge_index, index, "{path}");
            let points = match &connection.route {
                Route::Line { points } => {
                    assert!(
                        parents[connection.to].replace(connection.from).is_none(),
                        "{path}: multiple entries to {}",
                        connection.to
                    );
                    let endpoint = *points.last().unwrap();
                    let target = cards[connection.to].bounds;
                    assert!(
                        (endpoint.x == target.x
                            && (target.y..target.bottom()).contains(&endpoint.y))
                            || (endpoint.y == target.y
                                && (target.x..target.right()).contains(&endpoint.x)),
                        "{path}: {endpoint:?} misses {target:?}"
                    );
                    if points[0].x == endpoint.x
                        && let Some(label) = &connection.label
                    {
                        const MAX_LABEL_OFFSET: u32 = 16;
                        assert!(
                            label.bounds.x > endpoint.x,
                            "{path}: vertical line crosses label {:?}",
                            label.text
                        );
                        assert!(
                            label.bounds.x - endpoint.x <= MAX_LABEL_OFFSET,
                            "{path}: label {:?} drifts away from its line",
                            label.text
                        );
                    }
                    points
                }
                Route::Jump {
                    points,
                    origin,
                    kind,
                    target,
                } => {
                    labels.push((index, jump_bounds(*origin, *kind, target)));
                    points
                }
            };
            segments.extend(points.windows(2).map(|pair| Segment {
                start: pair[0],
                end: pair[1],
                from: connection.from,
                edge_index: Some(index),
            }));
            if let Some(label) = &connection.label {
                labels.push((index, label.bounds));
            }
        }
        for connection in &layout.connections {
            if let Route::Jump { kind, .. } = connection.route {
                assert_eq!(
                    kind == JumpKind::Back,
                    is_ancestor(&parents, connection.from, connection.to),
                    "{path}: incorrect jump {connection:?}"
                );
            }
        }
        (segments, labels)
    }

    fn is_ancestor(parents: &[Option<usize>], mut node: usize, target: usize) -> bool {
        for _ in 0..parents.len() {
            if node == target {
                return true;
            }
            let Some(parent) = parents[node] else {
                return false;
            };
            node = parent;
        }
        panic!("cycle among line routes");
    }

    fn assert_headings_clear(
        path: &str,
        diagram: &Diagram,
        layout: &Layout,
        labels: &[(usize, Rect)],
        segments: &[Segment],
    ) {
        const HEADING_DESCENT: u32 = 4;
        for band in &layout.sections {
            let (Some(cluster), Some(heading)) = (band.cluster_index, band.heading) else {
                continue;
            };
            let bounds = Rect {
                x: heading.x,
                y: heading.y - SECTION_FONT_SIZE,
                width: text_width(
                    &diagram.clusters[cluster].title,
                    SECTION_FONT_SIZE,
                    SemiBold,
                ),
                height: SECTION_FONT_SIZE + HEADING_DESCENT,
            };
            for segment in segments {
                assert!(
                    !segment.enters(bounds),
                    "{path}: {segment:?} crosses heading {bounds:?}"
                );
            }
            for &(_, label) in labels {
                assert!(
                    !overlaps(bounds, label),
                    "{path}: heading {bounds:?} overlaps label {label:?}"
                );
            }
            for card in &layout.cards {
                assert!(
                    !overlaps(bounds, card.bounds),
                    "{path}: heading {bounds:?} overlaps {card:?}"
                );
            }
        }
    }

    fn node_card<'a>(image: &'a Layout, diagram: &Diagram, id: &str) -> &'a Card {
        image
            .cards
            .iter()
            .find(|card| diagram.nodes[card.node_index].id == id)
            .unwrap()
    }

    #[derive(Clone, Copy, Debug)]
    struct Segment {
        start: Point,
        end: Point,
        from: usize,
        edge_index: Option<usize>,
    }

    impl Segment {
        fn vertical(self) -> bool {
            self.start.x == self.end.x
        }

        fn crosses(self, other: Self) -> bool {
            match (self.vertical(), other.vertical()) {
                (true, true) => {
                    self.start.x == other.start.x
                        && self.start.y < other.end.y
                        && self.end.y > other.start.y
                }
                (false, false) => {
                    self.start.y == other.start.y
                        && self.start.x < other.end.x
                        && self.end.x > other.start.x
                }
                (true, false) => self.crosses_horizontal(other),
                (false, true) => other.crosses_horizontal(self),
            }
        }

        fn crosses_horizontal(self, horizontal: Self) -> bool {
            let junction = Point {
                x: self.start.x,
                y: horizontal.start.y,
            };
            let is_joint = self.from == horizontal.from
                && [self.start, self.end, horizontal.start, horizontal.end].contains(&junction);
            if is_joint {
                return false;
            }
            (horizontal.start.x..=horizontal.end.x).contains(&junction.x)
                && (self.start.y..=self.end.y).contains(&junction.y)
        }

        fn enters(self, card: Rect) -> bool {
            if self.vertical() {
                return self.start.x > card.x
                    && self.start.x < card.right()
                    && self.end.y > card.y
                    && self.start.y < card.bottom();
            }
            self.start.y > card.y
                && self.start.y < card.bottom()
                && self.end.x > card.x
                && self.start.x < card.right()
        }
    }

    fn assert_segments_clear(path: &str, segments: &[Segment], cards: &[Card]) {
        for (index, segment) in segments.iter().copied().enumerate() {
            assert!(
                segment.vertical() || segment.start.y == segment.end.y,
                "{path}: diagonal {segment:?}"
            );
            assert!(
                segment.start.x <= segment.end.x && segment.start.y <= segment.end.y,
                "{path}: backwards {segment:?}"
            );
            for &other in &segments[index + 1..] {
                assert!(
                    !segment.crosses(other),
                    "{path}: {segment:?} crosses {other:?}"
                );
            }
            for card in cards {
                assert!(
                    !segment.enters(card.bounds),
                    "{path}: {segment:?} enters {card:?}"
                );
            }
        }
    }

    fn assert_labels_clear(
        path: &str,
        labels: &[(usize, Rect)],
        segments: &[Segment],
        layout: &Layout,
        cards: &[&Card],
    ) {
        for (index, &(edge_index, bounds)) in labels.iter().enumerate() {
            assert!(
                bounds.right() < layout.width && bounds.bottom() < layout.height,
                "{path}: clipped label {bounds:?}"
            );
            let source = cards[layout.connections[edge_index].from];
            let band = layout.sections[source.band_index].bounds;
            assert!(
                encloses(band, bounds),
                "{path}: label {bounds:?} is outside source band {band:?}"
            );
            for section in &layout.sections {
                if overlaps(section.bounds, bounds) {
                    assert!(
                        encloses(section.bounds, bounds),
                        "{path}: label {bounds:?} intersects a band border {section:?}"
                    );
                }
            }
            for card in &layout.cards {
                assert!(
                    !overlaps(bounds, card.bounds),
                    "{path}: label {bounds:?} overlaps {card:?}"
                );
            }
            for &(_, other) in &labels[index + 1..] {
                assert!(
                    !overlaps(bounds, other),
                    "{path}: label {bounds:?} overlaps {other:?}"
                );
            }
            for &segment in segments {
                if segment.edge_index != Some(edge_index) {
                    assert!(
                        !segment.enters(bounds),
                        "{path}: {segment:?} crosses label {bounds:?}"
                    );
                }
            }
        }
    }

    fn encloses(outer: Rect, inner: Rect) -> bool {
        outer.x <= inner.x
            && inner.right() <= outer.right()
            && outer.y <= inner.y
            && inner.bottom() <= outer.bottom()
    }

    fn overlaps(left: Rect, right: Rect) -> bool {
        left.x < right.right()
            && left.right() > right.x
            && left.y < right.bottom()
            && left.bottom() > right.y
    }
}
