use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use unicode_width::UnicodeWidthStr;

use crate::flows::{self, CatalogError, Flow};
use crate::{Options, markdown};

/// Journey order belongs to the catalog author rather than the source code.
#[derive(Clone, Debug)]
pub struct Map {
    pub file: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub activities: Vec<Activity>,
}

/// A user's activity can exist before any of its flows have been documented.
#[derive(Clone, Debug)]
pub struct Activity {
    pub name: String,
    pub description: Option<String>,
    pub line: usize,
    pub entries: Vec<MapEntry>,
}

/// Source lines let validation point to the link that needs attention.
#[derive(Clone, Debug)]
pub struct MapEntry {
    pub text: String,
    pub target: String,
    pub line: usize,
}

/// An absent activity collects flows the author has not placed yet.
#[derive(Debug)]
pub struct Group<'a> {
    pub activity: Option<&'a Activity>,
    pub flows: Vec<&'a Flow>,
}

/// Layout selection is separate from rendering so callers control terminal sizing.
#[derive(Clone, Copy, Debug)]
pub enum MapLayout {
    Grid,
    Outline,
}

pub const UNMAPPED_HEADING: &str = "Not on the map";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub activity: String,
    /// Index into `Map::activities`; `group_flows` preserves activity order.
    pub activity_index: usize,
    /// 1-based position within the activity.
    pub position: usize,
    /// Number of flows in the activity.
    pub count: usize,
}

/// Box and title geometry share one column width.
pub(crate) const GRID_COLUMN: usize = 24;
/// The gap leaves room for an arrow between activity boxes.
pub(crate) const GRID_GAP: usize = 3;
/// Leave room for the box corners, borders and name padding.
const GRID_NAME_WIDTH: usize = GRID_COLUMN - 6;
/// Earlier title columns can borrow space from the following gap.
const GRID_WRAP: usize = GRID_COLUMN - 1;
/// Each title column starts after a box and its following gap.
const GRID_PITCH: usize = GRID_COLUMN + GRID_GAP;

/// Description and flow lines align under the activity name.
const OUTLINE_INDENT: usize = 3;
/// Leave room for corners, padding and the activity name.
const BOX_RULE_FILL: usize = GRID_COLUMN - 5;
/// Leave room for both borders and count padding.
const BOX_INNER_WIDTH: usize = GRID_COLUMN - 3;
/// Leave room for both bottom corners.
const BOX_BOTTOM_FILL: usize = GRID_COLUMN - 2;
/// Each box has a top border, count row and bottom border.
const BOX_ROWS: usize = 3;
/// The last title column and the indented unmapped list must fit within the grid border.
const GRID_LAST_WRAP: usize = GRID_WRAP - 1;
/// Below this, a shortened catalog path says nothing, so the whole header is shortened.
const MIN_CATALOG_COLUMNS: usize = 4;
/// Map links point at flow files in the catalog folder itself.
const FLOW_EXTENSIONS: [&str; 2] = ["md", "mmd"];

struct Glyphs {
    bullet: &'static str,
    arrow: &'static str,
    branch: &'static str,
    last_branch: &'static str,
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
    horizontal: &'static str,
    vertical: &'static str,
    ellipsis: &'static str,
}

const UNICODE_GLYPHS: Glyphs = Glyphs {
    bullet: "·",
    arrow: "▶",
    branch: "├─",
    last_branch: "└─",
    top_left: "╭",
    top_right: "╮",
    bottom_left: "╰",
    bottom_right: "╯",
    horizontal: "─",
    vertical: "│",
    ellipsis: "…",
};
const ASCII_GLYPHS: Glyphs = Glyphs {
    bullet: "-",
    arrow: ">",
    branch: "|-",
    last_branch: "`-",
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    horizontal: "-",
    vertical: "|",
    ellipsis: "...",
};

fn glyphs(opts: &Options) -> &'static Glyphs {
    glyphs_for(opts.ascii)
}

fn glyphs_for(ascii: bool) -> &'static Glyphs {
    if ascii {
        &ASCII_GLYPHS
    } else {
        &UNICODE_GLYPHS
    }
}

/// The separator and list bullet used by map output.
pub fn bullet(ascii: bool) -> &'static str {
    glyphs_for(ascii).bullet
}

impl MapLayout {
    pub fn fitting(activities: usize, width: usize) -> MapLayout {
        if grid_width(activities) <= width {
            Self::Grid
        } else {
            Self::Outline
        }
    }
}

pub fn grid_width(activities: usize) -> usize {
    1 + activities * GRID_COLUMN + activities.saturating_sub(1) * GRID_GAP
}

fn heading_text(rest: &str) -> String {
    let text = rest.trim();
    let without_hashes = text.trim_end_matches('#');
    if without_hashes.is_empty() || without_hashes.ends_with(' ') {
        without_hashes.trim().into()
    } else {
        text.into()
    }
}

struct MapParser {
    map: Map,
    saw_list: bool,
}

impl MapParser {
    fn read_line(&mut self, raw: &str, line: usize) {
        if let Some(name) = raw.strip_prefix("## ") {
            self.start_activity(name, line);
        } else if let Some(title) = raw.strip_prefix("# ") {
            self.set_title(title);
        } else if !raw.trim().is_empty() && !raw.starts_with('#') {
            self.read_content(raw, line);
        }
    }

    fn start_activity(&mut self, name: &str, line: usize) {
        self.map.activities.push(Activity {
            name: heading_text(name),
            description: None,
            line,
            entries: Vec::new(),
        });
        self.saw_list = false;
    }

    fn set_title(&mut self, title: &str) {
        if self.map.title.is_none() {
            self.map.title = Some(heading_text(title));
        }
    }

    fn read_content(&mut self, raw: &str, line: usize) {
        if let Some(activity) = self.map.activities.last_mut() {
            if let Some(item) = list_item(raw) {
                self.saw_list = true;
                if let Some(entry) = link_entry(item, line) {
                    activity.entries.push(entry);
                }
            } else if !self.saw_list && activity.description.is_none() {
                activity.description = Some(raw.trim().into());
            }
        } else if self.map.title.is_some()
            && self.map.description.is_none()
            && list_item(raw).is_none()
        {
            self.map.description = Some(raw.trim().into());
        }
    }
}

pub fn parse_map(file: &str, text: &str) -> Option<Map> {
    let normalised = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n");
    let mut parser = MapParser {
        map: Map {
            file: file.into(),
            title: None,
            description: None,
            activities: Vec::new(),
        },
        saw_list: false,
    };
    let mut active_fence = None;
    for (index, raw) in normalised.lines().enumerate() {
        if let Some(open) = &active_fence {
            if markdown::closes(open, raw) {
                active_fence = None;
            }
            continue;
        }
        if let Some(open) = markdown::fence(raw) {
            active_fence = Some(open);
            continue;
        }
        let line = index + 1;
        parser.read_line(raw, line);
    }
    parser
        .map
        .activities
        .iter()
        .any(|activity| !activity.entries.is_empty())
        .then_some(parser.map)
}

fn list_item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let after_marker = trimmed.strip_prefix(['-', '*', '+'])?;
    if !after_marker.starts_with(' ') {
        return None;
    }
    Some(after_marker.trim_start_matches(' '))
}

/// The link destination without `<…>`, a title, a leading `./` or a `#fragment`.
fn link_destination(inside: &str) -> Option<&str> {
    let destination = if let Some(bracketed) = inside.strip_prefix('<') {
        bracketed.split_once('>')?.0
    } else {
        inside.split_whitespace().next()?
    };
    let relative = destination.strip_prefix("./").unwrap_or(destination);
    let target = relative.split('#').next()?;
    if target.is_empty() {
        return None;
    }
    Some(target)
}

/// Only a flow file name in this folder can be a map entry.
fn is_catalog_flow_target(target: &str) -> bool {
    if target.contains([':', '/', '\\']) {
        return false;
    }
    let Some((_, extension)) = target.rsplit_once('.') else {
        return false;
    };
    FLOW_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
}

fn link_entry(item: &str, line: usize) -> Option<MapEntry> {
    let (text, rest) = item.strip_prefix('[')?.split_once("](")?;
    let (inside, _) = rest.split_once(')')?;
    let target = link_destination(inside)?;
    if !is_catalog_flow_target(target) {
        return None;
    }
    Some(MapEntry {
        text: text.into(),
        target: target.into(),
        line,
    })
}

fn pick_readme(names: &[String]) -> Option<&String> {
    names
        .iter()
        .find(|name| name.as_str() == "README.md")
        .or_else(|| {
            names
                .iter()
                .filter(|name| name.eq_ignore_ascii_case("readme.md"))
                .min()
        })
}

/// A README that cannot be read is treated as no map, as before maps existed.
pub fn load_map(dir: &Path) -> Result<Option<Map>, CatalogError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CatalogError {
                path: dir.into(),
                error,
            });
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| CatalogError {
            path: dir.into(),
            error,
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    let Some(file) = pick_readme(&names) else {
        return Ok(None);
    };
    Ok(read_readme_text(&dir.join(file)).and_then(|text| parse_map(file, &text)))
}

/// Any problem reading the README means no map: before maps existed, the README was ignored.
fn read_readme_text(path: &Path) -> Option<String> {
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    String::from_utf8(fs::read(path).ok()?).ok()
}

pub(crate) fn find_flow<'a>(flows: &'a [Flow], target: &str) -> Option<&'a Flow> {
    flows.iter().find(|flow| flow.path == target)
}

pub fn group_flows<'a>(map: &'a Map, flows: &'a [Flow]) -> Vec<Group<'a>> {
    let mut placed = HashSet::new();
    let mut groups = Vec::new();
    for activity in &map.activities {
        let mut members = Vec::new();
        for entry in &activity.entries {
            if let Some(flow) = find_flow(flows, &entry.target)
                && placed.insert(flow.path.as_str())
            {
                members.push(flow);
            }
        }
        groups.push(Group {
            activity: Some(activity),
            flows: members,
        });
    }
    let mut remaining: Vec<_> = flows
        .iter()
        .filter(|flow| !placed.contains(flow.path.as_str()))
        .collect();
    remaining.sort_by(|left, right| flows::compare_titles(left, right));
    if !remaining.is_empty() {
        groups.push(Group {
            activity: None,
            flows: remaining,
        });
    }
    groups
}

pub fn placements(groups: &[Group<'_>]) -> HashMap<String, Placement> {
    let mut placements = HashMap::new();
    for (activity_index, group) in groups.iter().enumerate() {
        if let Some(activity) = group.activity {
            for (index, flow) in group.flows.iter().enumerate() {
                placements.insert(
                    flow.path.clone(),
                    Placement {
                        activity: activity.name.clone(),
                        activity_index,
                        position: index + 1,
                        count: group.flows.len(),
                    },
                );
            }
        }
    }
    placements
}

fn styled(text: &str, sgr: &str, opts: &Options) -> String {
    if opts.color {
        format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
        text.into()
    }
}

fn display_text(text: &str, opts: &Options) -> String {
    if opts.ascii {
        text.chars()
            .map(|character| if character.is_ascii() { character } else { '?' })
            .collect()
    } else {
        text.into()
    }
}

fn counted(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn count_label(count: usize) -> String {
    counted(count, "use case", "use cases")
}

pub fn render_map(
    groups: &[Group<'_>],
    catalog: &str,
    layout: MapLayout,
    width: usize,
    opts: &Options,
) -> String {
    let total: usize = groups.iter().map(|group| group.flows.len()).sum();
    let activities = groups
        .iter()
        .filter(|group| group.activity.is_some())
        .count();
    let unmapped: usize = groups
        .iter()
        .filter(|group| group.activity.is_none())
        .map(|group| group.flows.len())
        .sum();
    let limit = match layout {
        MapLayout::Grid => width.max(grid_width(activities)),
        MapLayout::Outline => width,
    };
    let separator = glyphs(opts).bullet;
    let catalog_part = format!(" {}", display_text(catalog, opts));
    let mut counts = format!(
        " {separator} {} in {}",
        count_label(total),
        counted(activities, "activity", "activities")
    );
    if unmapped > 0 {
        counts.push_str(&format!(
            " {separator} {unmapped} {}",
            UNMAPPED_HEADING.to_lowercase()
        ));
    }
    let header = fit_header(&catalog_part, &counts, limit, opts.ascii);
    let mut output = format!("{}\n\n", styled(&header, "2", opts));
    match layout {
        MapLayout::Grid => output.push_str(&render_grid(groups, opts)),
        MapLayout::Outline => output.push_str(&render_outline(groups, width, opts)),
    }
    output
}

/// The counts say whether the map has drifted, so the catalog path is shortened first.
fn fit_header(catalog_part: &str, counts: &str, limit: usize, ascii: bool) -> String {
    let header = format!("{catalog_part}{counts}");
    let catalog_width = limit.saturating_sub(counts.width());
    if header.width() > limit && catalog_width >= MIN_CATALOG_COLUMNS {
        format!("{}{counts}", truncate(catalog_part, catalog_width, ascii))
    } else {
        truncate(&header, limit, ascii)
    }
}

fn outline_activity_title(number: usize, name: &str, width: usize, opts: &Options) -> String {
    let prefix = format!(" {number} ");
    let title = if width < prefix.width() {
        styled(&truncate(&prefix, width, opts.ascii), "33", opts)
    } else {
        let name = truncate(
            &display_text(name, opts),
            width.saturating_sub(prefix.width()),
            opts.ascii,
        );
        format!(
            " {} {}",
            styled(&number.to_string(), "33", opts),
            styled(&name, "1;36", opts)
        )
    };
    format!("{title}\n")
}

fn outline_description(description: &str, width: usize, opts: &Options) -> String {
    let indent = " ".repeat(OUTLINE_INDENT);
    wrap_words(
        &display_text(description, opts),
        width.saturating_sub(OUTLINE_INDENT).max(1),
    )
    .into_iter()
    .map(|line| {
        let fitted_line = truncate(&format!("{indent}{line}"), width, opts.ascii);
        format!("{}\n", styled(&fitted_line, "2", opts))
    })
    .collect()
}

fn outline_unmapped_heading(width: usize, opts: &Options) -> String {
    format!(
        "{}\n",
        styled(
            &truncate(&format!(" {UNMAPPED_HEADING}"), width, opts.ascii),
            "1;36",
            opts
        )
    )
}

fn outline_flow_lines(group: &Group<'_>, width: usize, opts: &Options) -> String {
    let symbols = glyphs(opts);
    let indent = " ".repeat(OUTLINE_INDENT);
    let mut output = String::new();
    for (index, flow) in group.flows.iter().enumerate() {
        let branch = if index + 1 == group.flows.len() {
            symbols.last_branch
        } else {
            symbols.branch
        };
        let line = truncate(
            &format!("{indent}{branch} {}", display_text(&flow.title, opts)),
            width,
            opts.ascii,
        );
        output.push_str(&format!("{line}\n"));
    }
    output
}

fn render_outline(groups: &[Group<'_>], width: usize, opts: &Options) -> String {
    let mut sections = Vec::new();
    let mut number = 0;
    for group in groups {
        let mut section = if let Some(activity) = group.activity {
            number += 1;
            let mut lines = outline_activity_title(number, &activity.name, width, opts);
            if let Some(description) = &activity.description {
                lines.push_str(&outline_description(description, width, opts));
            }
            lines
        } else {
            outline_unmapped_heading(width, opts)
        };
        section.push_str(&outline_flow_lines(group, width, opts));
        sections.push(section);
    }
    sections.join("\n")
}

pub fn truncate(text: &str, width: usize, ascii: bool) -> String {
    if text.width() <= width {
        return text.into();
    }
    let suffix = glyphs_for(ascii).ellipsis;
    if width < suffix.width() {
        return suffix.chars().take(width).collect();
    }
    let budget = width - suffix.width();
    let mut result = String::new();
    for character in text.chars() {
        result.push(character);
        if result.width() > budget {
            result.pop();
            break;
        }
    }
    result.push_str(suffix);
    result
}

fn padded(text: &str, visible_width: usize, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(visible_width)))
}

fn grid_box(group: &Group<'_>, opts: &Options) -> [String; BOX_ROWS] {
    let name = group
        .activity
        .map(|activity| display_text(&activity.name, opts))
        .unwrap_or_default();
    let name = truncate(&name, GRID_NAME_WIDTH, opts.ascii);
    let Glyphs {
        top_left,
        top_right,
        bottom_left,
        bottom_right,
        horizontal,
        vertical,
        ..
    } = glyphs(opts);
    let count = count_label(group.flows.len());
    [
        format!(
            "{top_left}{horizontal} {} {}{top_right}",
            styled(&name, "1;36", opts),
            horizontal.repeat(BOX_RULE_FILL.saturating_sub(name.width()))
        ),
        format!(
            "{vertical} {}{vertical}",
            padded(&styled(&count, "2", opts), count.width(), BOX_INNER_WIDTH)
        ),
        format!(
            "{bottom_left}{}{bottom_right}",
            horizontal.repeat(BOX_BOTTOM_FILL)
        ),
    ]
}

fn wrap_words(text: &str, limit: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && format!("{current} {word}").width() > limit {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        for character in word.chars() {
            current.push(character);
            if current.width() > limit {
                current.pop();
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                current.push(character);
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn wrap_title(title: &str, limit: usize, opts: &Options) -> Vec<String> {
    wrap_words(&display_text(title, opts), limit)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{} {line}", glyphs(opts).bullet)
            } else {
                format!("  {line}")
            }
        })
        .collect()
}

fn grid_box_rows(activities: &[&Group<'_>], opts: &Options) -> String {
    let boxes: Vec<_> = activities
        .iter()
        .map(|group| grid_box(group, opts))
        .collect();
    let mut output = String::new();
    for row in 0..BOX_ROWS {
        let gap = if row == 1 {
            format!(" {} ", styled(glyphs(opts).arrow, "33", opts))
        } else {
            " ".repeat(GRID_GAP)
        };
        output.push_str(&format!(
            " {}\n",
            boxes
                .iter()
                .map(|lines| lines[row].as_str())
                .collect::<Vec<_>>()
                .join(&gap)
        ));
    }
    output
}

fn column_limit(index: usize, count: usize) -> usize {
    // Earlier columns can use the following gap; the last must fit the grid border.
    if index + 1 == count {
        GRID_LAST_WRAP
    } else {
        GRID_WRAP
    }
}

fn grid_title_columns(activities: &[&Group<'_>], opts: &Options) -> String {
    let columns: Vec<Vec<_>> = activities
        .iter()
        .enumerate()
        .map(|(index, group)| {
            group
                .flows
                .iter()
                .flat_map(|flow| {
                    wrap_title(&flow.title, column_limit(index, activities.len()), opts)
                })
                .collect()
        })
        .collect();
    let height = columns.iter().map(Vec::len).max().unwrap_or(0);
    let mut output = String::new();
    for row in 0..height {
        let mut line = String::new();
        for column in &columns {
            line.push(' ');
            let cell = column.get(row).map(String::as_str).unwrap_or("");
            line.push_str(&padded(cell, cell.width(), GRID_PITCH - 1));
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

fn grid_unmapped(groups: &[Group<'_>], opts: &Options) -> String {
    let mut output = String::new();
    for group in groups.iter().filter(|group| group.activity.is_none()) {
        output.push_str(&format!("\n {}\n", styled(UNMAPPED_HEADING, "1;36", opts)));
        for flow in &group.flows {
            for line in wrap_title(&flow.title, GRID_LAST_WRAP, opts) {
                output.push_str(&format!(" {line}\n"));
            }
        }
    }
    output
}

fn render_grid(groups: &[Group<'_>], opts: &Options) -> String {
    let activities: Vec<_> = groups
        .iter()
        .filter(|group| group.activity.is_some())
        .collect();
    let mut output = grid_box_rows(&activities, opts);
    output.push_str(&grid_title_columns(&activities, opts));
    output.push_str(&grid_unmapped(groups, opts));
    output
}

pub fn render_grouped_list(groups: &[Group<'_>], opts: &Options) -> String {
    let width = groups
        .iter()
        .flat_map(|group| &group.flows)
        .map(|flow| flow.name.len())
        .max()
        .unwrap_or(0);
    let mut sections = Vec::new();
    for group in groups {
        let name = group
            .activity
            .map(|activity| activity.name.as_str())
            .unwrap_or(UNMAPPED_HEADING);
        let mut section = crate::render::bold_line(name, opts.color);
        for flow in &group.flows {
            section.push_str(&format!("  {}", flows::list_line(flow, width)));
        }
        sections.push(section);
    }
    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_targets_are_cleaned_and_filtered() {
        for (item, expected) in [
            (r#"[a](a.md "Title")"#, Some("a.md")),
            ("[a](<a.md>)", Some("a.md")),
            ("[a](<my flow.md>)", Some("my flow.md")),
            ("[a](./a.md#x)", Some("a.md")),
            ("[a](A.MMD)", Some("A.MMD")),
            ("[s](https://x.io)", None),
            ("[d](../a.md)", None),
            ("[m](mailto:x)", None),
            ("[p](a.txt)", None),
            (r"[d](dir\a.md)", None),
        ] {
            assert_eq!(
                link_entry(item, 1).map(|entry| entry.target),
                expected.map(str::to_owned),
                "{item}"
            );
        }
    }

    #[test]
    fn readme_with_only_foreign_links_is_no_map() {
        assert!(
            parse_map(
                "README.md",
                concat!(
                    "## Docs\n",
                    "- [s](https://x.io)\n",
                    "- [d](../a.md)\n",
                    "- [m](mailto:x)\n",
                    "- [p](a.txt)\n"
                )
            )
            .is_none()
        );
    }

    #[test]
    fn heading_text_strips_closing_hashes() {
        let map = parse_map(
            "README.md",
            concat!("## C#\n", "- [a](a.md)\n", "## Find ##\n"),
        )
        .unwrap();
        assert_eq!(map.activities[0].name, "C#");
        assert_eq!(map.activities[1].name, "Find");
    }

    fn one_activity_grid_with_unmapped(title: &str, width: usize) -> String {
        let map = parse_map("README.md", "## Activity\n- [X](x.md)\n").unwrap();
        let flows = vec![flow("x.md", "X"), flow("y.md", title)];
        render_map(
            &group_flows(&map, &flows),
            "flows",
            MapLayout::Grid,
            width,
            &Options::default(),
        )
    }

    #[test]
    fn grid_unmapped_flows_align_with_their_heading() {
        let output =
            one_activity_grid_with_unmapped("An unmapped flow with a title that wraps", 80);
        assert!(
            output
                .ends_with("\n Not on the map\n · An unmapped flow with\n   a title that wraps\n"),
            "{output}"
        );
    }

    #[test]
    fn unmapped_flows_fit_a_one_activity_grid() {
        // "An unmapped flow with a" fills the 23-column wrap that overflowed by one.
        let output = one_activity_grid_with_unmapped("An unmapped flow with a title that wraps", 0);
        assert!(
            output.lines().all(|line| line.width() <= grid_width(1)),
            "{output}"
        );
    }

    #[test]
    fn narrow_outline_wraps_description_words() {
        let map = parse_map(
            "README.md",
            "## Activity\nA description that needs wrapping.\n- [X](x.md)\n",
        )
        .unwrap();
        let flows = vec![flow("x.md", "X")];
        let output = render_map(
            &group_flows(&map, &flows),
            "flows",
            MapLayout::Outline,
            20,
            &Options::default(),
        );
        assert!(
            output.contains("   A description\n   that needs\n   wrapping.\n"),
            "{output}"
        );
    }

    #[test]
    fn outline_fits_even_below_the_heading_indent() {
        let map = parse_map(
            "README.md",
            concat!(
                "## A long activity name\n",
                "A description that needs wrapping.\n",
                "- [X](x.md)\n"
            ),
        )
        .unwrap();
        let flows = vec![flow("x.md", "A long title"), flow("y.md", "Unmapped")];
        let groups = group_flows(&map, &flows);
        for ascii in [false, true] {
            let opts = Options {
                ascii,
                ..Options::default()
            };
            for width in 0..=40 {
                let output = render_map(&groups, "flows", MapLayout::Outline, width, &opts);
                assert!(
                    output.lines().all(|line| line.width() <= width),
                    "width {width}: {output}"
                );
            }
        }
    }

    #[test]
    fn vs16_and_zwj_names_never_overflow() {
        for name in ["❤️".repeat(20), "👩‍💻".repeat(20)] {
            assert!(truncate(&name, 18, false).width() <= 18);
            let map = parse_map(
                "README.md",
                &format!(concat!("## {}\n", "- [X](x.md)\n"), name),
            )
            .unwrap();
            let flows = vec![flow("x.md", &name)];
            let output = render_grid(&group_flows(&map, &flows), &Options::default());
            assert!(
                output.lines().all(|line| line.width() <= grid_width(1)),
                "{output}"
            );
        }
    }

    #[test]
    fn parses_activities_links_and_descriptions() {
        let text = concat!(
            "\u{feff}# Shop\r\n",
            "\r\n",
            "The journey.\r\n",
            "## Find ###\r\n",
            "Find something.\r\n",
            "- [First](./x.md) extra\r\n",
            "````markdown\r\n",
            "## Fake\r\n",
            "```\r\n",
            "- [Fake](fake.md)\r\n",
            "````\r\n",
            "## Buy\r\n",
            "Pay for it.\r\n",
            "### Details\r\n",
            "  * [Again](x.md#a)\r\n",
            "~~~\r\n",
            "## Also fake\r\n",
            "~~~~\r\n"
        );
        let map = parse_map("ReadMe.md", text).unwrap();
        assert_eq!(map.file, "ReadMe.md");
        assert_eq!(map.title.as_deref(), Some("Shop"));
        assert_eq!(map.description.as_deref(), Some("The journey."));
        assert_eq!(map.activities.len(), 2);
        let first = &map.activities[0];
        assert_eq!(first.name, "Find");
        assert_eq!(first.description.as_deref(), Some("Find something."));
        assert_eq!(first.line, 4);
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].text, "First");
        assert_eq!(first.entries[0].target, "x.md");
        assert_eq!(first.entries[0].line, 6);
        let second = &map.activities[1];
        assert_eq!(second.name, "Buy");
        assert_eq!(second.description.as_deref(), Some("Pay for it."));
        assert_eq!(second.line, 12);
        assert_eq!(second.entries[0].target, "x.md");
        assert_eq!(second.entries[0].line, 15);
    }

    #[test]
    fn readme_without_links_is_not_a_map() {
        for text in [
            concat!("# Readme\n", "Description.\n", "## Setup\n", "Run cargo.\n"),
            "# Readme\n- [Outside](x.md)",
            concat!("## Setup\n", "    - [Indented](x.md)\n"),
            concat!("## Setup\n", "```\n", "- [Fenced](x.md)"),
        ] {
            assert!(parse_map("README.md", text).is_none());
        }
    }

    #[test]
    fn descriptions_stop_at_any_list_item() {
        let map = parse_map(
            "README.md",
            concat!(
                "# Map\n",
                "- notes\n",
                "Map description.\n",
                "## First\n",
                "- notes\n",
                "Not a description.\n",
                "- [X](x.md)\n",
                "## Empty\n",
                "A future activity.\n"
            ),
        )
        .unwrap();
        assert_eq!(map.description.as_deref(), Some("Map description."));
        assert!(map.activities[0].description.is_none());
        assert_eq!(
            map.activities[1].description.as_deref(),
            Some("A future activity.")
        );
    }

    fn flow(path: &str, title: &str) -> Flow {
        flows::parse_flow(path, &format!("# {title}\n"))
    }

    #[test]
    fn groups_follow_map_order_then_unmapped_by_title() {
        let map = parse_map(
            "README.md",
            concat!(
                "## Second\n",
                "- [B](b.md)\n",
                "- [A](a.md)\n",
                "- [Missing](missing.md)\n",
                "## First\n",
                "- [B](b.md)\n",
                "## Empty\n"
            ),
        )
        .unwrap();
        let flows = vec![
            flow("a.md", "A"),
            flow("b.md", "B"),
            flow("z.md", "alpha"),
            flow("c.md", "Zulu"),
        ];
        let groups = group_flows(&map, &flows);
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[0].activity.unwrap().name, "Second");
        assert_eq!(
            groups[0]
                .flows
                .iter()
                .map(|flow| flow.path.as_str())
                .collect::<Vec<_>>(),
            ["b.md", "a.md"]
        );
        assert!(groups[1].flows.is_empty());
        assert!(groups[2].flows.is_empty());
        assert!(groups[3].activity.is_none());
        assert_eq!(
            groups[3]
                .flows
                .iter()
                .map(|flow| flow.path.as_str())
                .collect::<Vec<_>>(),
            ["z.md", "c.md"]
        );
        let placements = placements(&groups);
        assert_eq!(
            placements.get("a.md"),
            Some(&Placement {
                activity: "Second".into(),
                activity_index: 0,
                position: 2,
                count: 2
            })
        );
        assert!(!placements.contains_key("z.md"));
    }

    #[test]
    fn pick_readme_prefers_exact_name() {
        for (names, expected) in [
            (vec!["readme.md", "README.md", "Readme.md"], "README.md"),
            (vec!["readme.md", "Readme.md"], "Readme.md"),
        ] {
            let names: Vec<_> = names.into_iter().map(str::to_owned).collect();
            assert_eq!(pick_readme(&names).map(String::as_str), Some(expected));
        }
    }

    #[test]
    fn loads_actual_readme_name() {
        let directory = assert_fs::TempDir::new().unwrap();
        fs::write(
            directory.path().join("ReadMe.MD"),
            concat!("## Activity\n", "- [X](x.md)\n"),
        )
        .unwrap();
        assert_eq!(
            load_map(directory.path()).unwrap().unwrap().file,
            "ReadMe.MD"
        );
    }

    #[test]
    fn unreadable_or_absent_readme_is_no_map() {
        let directory = assert_fs::TempDir::new().unwrap();
        let invalid_utf8 = directory.path().join("invalid-utf8");
        fs::create_dir(&invalid_utf8).unwrap();
        fs::write(invalid_utf8.join("README.md"), [0xff]).unwrap();
        let readme_directory = directory.path().join("readme-directory");
        fs::create_dir_all(readme_directory.join("README.md")).unwrap();
        let missing_catalog = directory.path().join("missing");
        for (label, path) in [
            ("invalid UTF-8", invalid_utf8),
            ("missing catalog", missing_catalog),
            ("README is a directory", readme_directory),
        ] {
            assert!(load_map(&path).unwrap().is_none(), "{label}");
        }
    }

    #[test]
    fn catalog_read_errors_propagate() {
        let directory = assert_fs::TempDir::new().unwrap();
        let path = directory.path().join("file");
        fs::write(&path, "not a directory").unwrap();
        assert_eq!(load_map(&path).unwrap_err().path, path);
    }

    #[test]
    fn grid_wraps_and_truncates_by_display_width() {
        let map = parse_map(
            "README.md",
            "## 商品を探して選んで購入する活動\n- [Title](x.md)",
        )
        .unwrap();
        let flows = vec![flow("x.md", &"界".repeat(20))];
        let groups = group_flows(&map, &flows);
        let output = render_map(&groups, "flows", MapLayout::Grid, 80, &Options::default());
        assert!(output.contains('…'));
        assert!(
            output
                .lines()
                .skip(2)
                .all(|line| line.width() <= grid_width(1)),
            "{output}"
        );
        let wrapped = wrap_title(&"a".repeat(40), GRID_WRAP, &Options::default());
        assert_eq!(
            wrapped,
            [
                format!("· {}", "a".repeat(23)),
                format!("  {}", "a".repeat(17))
            ]
        );
    }

    #[test]
    fn grid_cells_fit_even_when_words_use_the_whole_column() {
        let flows = vec![
            flow("a.md", &"a".repeat(40)),
            flow("b.md", &"界".repeat(20)),
            flow("c.md", "A title with several words to wrap"),
        ];
        for (text, count) in [
            (concat!("## First\n", "- [A](a.md)\n"), 1),
            (
                concat!(
                    "## First\n",
                    "- [A](a.md)\n",
                    "## Second\n",
                    "- [B](b.md)\n"
                ),
                2,
            ),
            (
                concat!(
                    "## First\n",
                    "- [A](a.md)\n",
                    "## Second\n",
                    "- [B](b.md)\n",
                    "## Third\n",
                    "- [C](c.md)\n"
                ),
                3,
            ),
        ] {
            let map = parse_map("README.md", text).unwrap();
            let groups = group_flows(&map, &flows[..count]);
            let output = render_map(&groups, "flows", MapLayout::Grid, 80, &Options::default());
            assert!(
                output
                    .lines()
                    .skip(2)
                    .all(|line| line.width() <= grid_width(count)),
                "{output}"
            );
        }
    }

    #[test]
    fn ascii_map_is_pure_ascii() {
        let map = parse_map(
            "README.md",
            concat!(
                "## A long activity name to truncate\n",
                "- [X](x.md)\n",
                "## 中文\n",
                "- [Y](y.md)"
            ),
        )
        .unwrap();
        let flows = vec![
            flow(
                "x.md",
                "A title with a long word aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            flow("y.md", "中文"),
            flow("z.md", "Unmapped"),
        ];
        for layout in [MapLayout::Grid, MapLayout::Outline] {
            for color in [false, true] {
                let output = render_map(
                    &group_flows(&map, &flows),
                    "flows",
                    layout,
                    80,
                    &Options {
                        ascii: true,
                        color,
                        ..Options::default()
                    },
                );
                assert!(output.is_ascii(), "{output}");
            }
        }
    }

    #[test]
    fn map_colors_and_unmapped_counts() {
        let map = parse_map(
            "README.md",
            concat!("## Activity\n", "Description.\n", "- [X](x.md)"),
        )
        .unwrap();
        let flows = vec![flow("x.md", "X"), flow("y.md", "Y")];
        let groups = group_flows(&map, &flows);
        let output = render_map(
            &groups,
            "flows",
            MapLayout::Outline,
            80,
            &Options {
                color: true,
                ..Options::default()
            },
        );
        assert!(output.starts_with(
            "\x1b[2m flows · 2 use cases in 1 activity · 1 not on the map\x1b[0m\n\n"
        ));
        assert!(
            output.contains(
                "\x1b[33m1\x1b[0m \x1b[1;36mActivity\x1b[0m\n\x1b[2m   Description.\x1b[0m"
            )
        );
        assert!(output.contains(UNMAPPED_HEADING));
        let list = render_grouped_list(&groups, &Options::default());
        assert_eq!(
            list,
            "Activity\n  x  X  (no mermaid block)\n\nNot on the map\n  y  Y  (no mermaid block)\n"
        );
    }
}
