use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::anchors::{self, Target};
use crate::diff::{self, DiagramDiff};
use crate::render::bold_line;
use crate::{Diagram, Options, ParseError, canvas, markdown, parser, paths};

pub const SHORT_SHA_LEN: usize = 7;
pub const MAX_STALE_FILES: usize = 5;

#[derive(Clone, Debug)]
pub struct Flow {
    pub path: String,
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub line_offset: usize,
}

pub fn parse_flow(path: &str, text: &str) -> Flow {
    let name = Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if is_mmd(path) {
        return Flow {
            path: path.to_string(),
            title: name.clone(),
            name,
            description: None,
            source: Some(text.to_string()),
            line_offset: 0,
        };
    }
    let outline = markdown::outline(text);
    let block = outline
        .blocks
        .into_iter()
        .find(|block| markdown::is_mermaid(&block.info));
    Flow {
        path: path.to_string(),
        title: outline
            .title
            .map(|(_, title)| title)
            .unwrap_or_else(|| name.clone()),
        name,
        description: outline.description.map(|(_, description)| description),
        line_offset: block.as_ref().map(|block| block.line).unwrap_or(0),
        source: block.map(|block| block.source),
    }
}

/// Errors include the opening fence offset so their lines refer to the file.
pub fn diagram(flow: &Flow) -> Option<Result<Diagram, ParseError>> {
    flow.source
        .as_ref()
        .map(|source| parse_diagram(source, flow.line_offset))
}

pub(crate) fn parse_diagram(source: &str, line_offset: usize) -> Result<Diagram, ParseError> {
    parser::parse(source).map_err(|error| ParseError {
        line: line_offset + error.line,
        message: error.message,
    })
}

fn is_hidden_or_readme(name: &str) -> bool {
    name.starts_with('.') || name.eq_ignore_ascii_case("readme.md")
}

pub fn is_mmd(name: &str) -> bool {
    paths::has_extension(Path::new(name), &["mmd"])
}

pub fn is_flow_file_name(name: &str) -> bool {
    !is_hidden_or_readme(name) && paths::has_extension(Path::new(name), &["md", "mmd"])
}

#[derive(Debug, thiserror::Error)]
#[error("{}: {error}", path.display())]
pub struct CatalogError {
    pub path: PathBuf,
    pub error: std::io::Error,
}

fn file_error(path: &Path, error: std::io::Error) -> CatalogError {
    CatalogError {
        path: path.to_path_buf(),
        error,
    }
}

#[derive(Debug)]
pub enum Entry {
    Flow { name: String, text: String },
    Unreadable { name: String, error: std::io::Error },
    Ignored { name: String },
}

/// Skips hidden entries and README.md; ignores other non-flow names without statting them.
/// Flow-named directories are ignored, while metadata and read errors are unreadable entries.
/// Entries are sorted by name so catalog validation and loading agree on duplicate order.
pub fn read_catalog(dir: &Path) -> Result<Vec<Entry>, CatalogError> {
    let mut catalog = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|error| file_error(dir, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| file_error(dir, error))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden_or_readme(&name) {
            continue;
        }
        if !is_flow_file_name(&name) {
            catalog.push(Entry::Ignored { name });
            continue;
        }
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                catalog.push(Entry::Ignored { name });
                continue;
            }
            Err(error) => {
                catalog.push(Entry::Unreadable { name, error });
                continue;
            }
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => catalog.push(Entry::Flow { name, text }),
            Err(error) => catalog.push(Entry::Unreadable { name, error }),
        }
    }
    catalog.sort_by(|left, right| entry_name(left).cmp(entry_name(right)));
    Ok(catalog)
}

fn entry_name(entry: &Entry) -> &str {
    match entry {
        Entry::Flow { name, .. } | Entry::Unreadable { name, .. } | Entry::Ignored { name } => name,
    }
}

pub fn load_dir(dir: &Path) -> Result<Vec<Flow>, CatalogError> {
    let mut flows = Vec::new();
    for entry in read_catalog(dir)? {
        match entry {
            Entry::Flow { name, text } => flows.push(parse_flow(&name, &text)),
            // Rendering and diffing skip vanished files and dangling links for compatibility.
            Entry::Unreadable { error, .. } if error.kind() == std::io::ErrorKind::NotFound => {}
            Entry::Unreadable { name, error } => return Err(file_error(&dir.join(name), error)),
            Entry::Ignored { .. } => {}
        }
    }
    Ok(flows)
}

#[derive(Debug)]
pub enum FlowChange {
    New {
        flow: Flow,
        diff: DiagramDiff,
        base_broken: bool,
    },
    Removed {
        flow: Flow,
    },
    Modified {
        flow: Flow,
        diff: DiagramDiff,
    },
    Unchanged {
        flow: Flow,
    },
    Broken {
        flow: Flow,
        error: ParseError,
    },
}

fn compare_flow(old: Option<&Flow>, new: &Flow) -> FlowChange {
    let previous = old.and_then(diagram);
    let base_broken = matches!(&previous, Some(Err(_)));
    let previous = previous.and_then(Result::ok);
    let current = match diagram(new) {
        Some(Ok(diagram)) => diagram,
        Some(Err(error)) => {
            return FlowChange::Broken {
                flow: new.clone(),
                error,
            };
        }
        None if previous.is_some() => return FlowChange::Removed { flow: new.clone() },
        None => return FlowChange::Unchanged { flow: new.clone() },
    };
    let Some(previous) = previous else {
        return FlowChange::New {
            flow: new.clone(),
            diff: diff::all_added(&current),
            base_broken,
        };
    };
    let difference = diff::diff(&previous, &current);
    if difference.is_empty() {
        FlowChange::Unchanged { flow: new.clone() }
    } else {
        FlowChange::Modified {
            flow: new.clone(),
            diff: difference,
        }
    }
}

pub fn compare(old: &[Flow], new: &[Flow]) -> Vec<FlowChange> {
    let mut paths: Vec<&str> = old
        .iter()
        .chain(new)
        .map(|flow| flow.path.as_str())
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let mut changes = Vec::new();
    for path in paths {
        let previous = old.iter().find(|flow| flow.path == path);
        let current = new.iter().find(|flow| flow.path == path);
        match (previous, current) {
            (_, Some(current)) => changes.push(compare_flow(previous, current)),
            (Some(previous), None) => changes.push(FlowChange::Removed {
                flow: previous.clone(),
            }),
            (None, None) => {}
        }
    }
    changes
}

/// Changed code paths covered by one anchor target.
#[derive(Clone, Debug)]
pub struct StaleHit {
    pub target: Target,
    pub files: Vec<String>,
}

/// An untouched flow whose anchored code changed.
#[derive(Clone, Debug)]
pub struct Stale {
    pub flow: Flow,
    pub hits: Vec<StaleHit>,
}

/// Flows whose file did not change but whose anchors match a changed file. `catalog_prefix` is the catalog dir relative to the repo root ("" or "docs/flows/").
pub fn stale_flows(current: &[Flow], catalog_prefix: &str, changed: &[String]) -> Vec<Stale> {
    let changed_set: HashSet<&str> = changed.iter().map(String::as_str).collect();
    let mut stale = Vec::new();
    for flow in current {
        if flow_is_touched(&changed_set, catalog_prefix, flow) {
            continue;
        }
        let hits = stale_hits(known_anchors(flow), changed);
        if !hits.is_empty() {
            stale.push(Stale {
                flow: flow.clone(),
                hits,
            });
        }
    }
    stale.sort_by(|left, right| left.flow.path.cmp(&right.flow.path));
    stale
}

/// The flow's grouped anchors whose step exists in its diagram; whole-flow anchors always count.
fn known_anchors(flow: &Flow) -> Vec<anchors::Anchor> {
    let diagram = diagram(flow).and_then(Result::ok);
    anchors::grouped_anchors(flow)
        .into_iter()
        .filter(|anchor| match &anchor.target {
            Target::Flow => true,
            Target::Step(_) => diagram
                .as_ref()
                .is_some_and(|diagram| anchors::anchor_step_exists(diagram, &anchor.target)),
        })
        .collect()
}

fn flow_is_touched(changed: &HashSet<&str>, catalog_prefix: &str, flow: &Flow) -> bool {
    changed.contains(format!("{catalog_prefix}{}", flow.path).as_str())
}

fn stale_hits(anchors: Vec<anchors::Anchor>, changed: &[String]) -> Vec<StaleHit> {
    let mut hits = Vec::new();
    for anchor in anchors {
        let files = changed
            .iter()
            .filter(|path| anchors::any_pattern_matches(&anchor.patterns, path))
            .cloned()
            .collect();
        merge_hit(&mut hits, anchor.target, files);
    }
    for hit in &mut hits {
        hit.files.sort();
        hit.files.dedup();
    }
    hits.retain(|hit| !hit.files.is_empty());
    hits
}

fn merge_hit(hits: &mut Vec<StaleHit>, target: Target, mut files: Vec<String>) {
    if let Some(hit) = hits.iter_mut().find(|hit| hit.target == target) {
        hit.files.append(&mut files);
    } else {
        hits.push(StaleHit { target, files });
    }
}

#[derive(Debug)]
pub struct AnchoredFlow<'a> {
    pub flow: &'a Flow,
    pub steps: Vec<String>,
    pub whole_flow: bool,
}

/// Flows and existing steps whose valid anchors cover any supplied path.
pub fn anchored_flows<'a>(flows: &'a [Flow], files: &[String]) -> Vec<AnchoredFlow<'a>> {
    let mut matches = Vec::new();
    for flow in flows {
        let mut steps = Vec::new();
        let mut whole_flow = false;
        for anchor in known_anchors(flow) {
            let matches_file = files
                .iter()
                .any(|file| anchors::any_pattern_matches(&anchor.patterns, file));
            if !matches_file {
                continue;
            }
            match anchor.target {
                Target::Flow => whole_flow = true,
                Target::Step(id) => steps.push(id),
            }
        }
        if whole_flow || !steps.is_empty() {
            matches.push(AnchoredFlow {
                flow,
                steps,
                whole_flow,
            });
        }
    }
    matches.sort_by(|left, right| left.flow.path.cmp(&right.flow.path));
    matches
}

enum SectionKind {
    Modified,
    New { base_broken: bool },
}

fn render_diff_section(
    flow: &Flow,
    difference: &DiagramDiff,
    kind: SectionKind,
    opts: &Options,
) -> String {
    let (status, suffix) = match kind {
        SectionKind::Modified => ("modified", ""),
        SectionKind::New { base_broken: true } => ("new flow", " (base version did not parse)"),
        SectionKind::New { base_broken: false } => ("new flow", ""),
    };
    let mut output = bold_line(
        &format!(
            "{} ({})  {status}: {}{suffix}",
            flow.title,
            flow.path,
            difference.summary()
        ),
        opts.color,
    );
    if matches!(kind, SectionKind::Modified) {
        for (mark, detail) in difference.details() {
            if let Some(sgr) = canvas::mark_sgr(mark).filter(|_| opts.color) {
                output.push_str(&format!("  {sgr}{detail}\x1b[0m\n"));
            } else {
                output.push_str(&format!("  {detail}\n"));
            }
        }
    }
    output.push('\n');
    output.push_str(&crate::render_diagram(&difference.union, opts));
    output.push('\n');
    output
}

struct Counts {
    modified: usize,
    added: usize,
    removed: usize,
}

fn report_header(since: &str, short_sha: &str, counts: &Counts) -> String {
    format!(
        "flow changes since {since} ({short_sha}): {} modified, {} new, {} removed",
        counts.modified, counts.added, counts.removed
    )
}

fn title_block(text: &str, color: bool) -> String {
    bold_line(text, color) + "\n"
}

fn render_change(change: &FlowChange, opts: &Options) -> String {
    match change {
        FlowChange::Modified { flow, diff } => {
            render_diff_section(flow, diff, SectionKind::Modified, opts)
        }
        FlowChange::New {
            flow,
            diff,
            base_broken,
        } => render_diff_section(
            flow,
            diff,
            SectionKind::New {
                base_broken: *base_broken,
            },
            opts,
        ),
        FlowChange::Removed { flow } => title_block(
            &format!("{} ({})  removed", flow.title, flow.path),
            opts.color,
        ),
        FlowChange::Broken { flow, error } => title_block(
            &format!("{} ({})  parse error, {error}", flow.title, flow.path),
            opts.color,
        ),
        FlowChange::Unchanged { .. } => String::new(),
    }
}

fn render_changes(changes: &[FlowChange], since: &str, base: &str, opts: &Options) -> String {
    let mut counts = Counts {
        modified: 0,
        added: 0,
        removed: 0,
    };
    let mut broken = 0;
    let mut unchanged = Vec::new();
    for change in changes {
        match change {
            FlowChange::Modified { .. } => counts.modified += 1,
            FlowChange::New { .. } => counts.added += 1,
            FlowChange::Removed { .. } => counts.removed += 1,
            FlowChange::Broken { .. } => broken += 1,
            FlowChange::Unchanged { flow } => unchanged.push(flow.name.as_str()),
        }
    }
    let short_sha: String = base.chars().take(SHORT_SHA_LEN).collect();
    if counts.modified + counts.added + counts.removed + broken == 0 {
        return format!("no flow changes since {since} ({short_sha})\n");
    }
    let mut output = bold_line(&report_header(since, &short_sha, &counts), opts.color);
    output.push('\n');
    for change in changes {
        output.push_str(&render_change(change, opts));
    }
    if !unchanged.is_empty() {
        output.push_str(&format!("unchanged: {}\n", unchanged.join(", ")));
    }
    output
}

pub fn render_report(
    changes: &[FlowChange],
    stale: &[Stale],
    since: &str,
    base: &str,
    opts: &Options,
) -> String {
    let mut output = render_changes(changes, since, base, opts);
    if stale.is_empty() {
        return output;
    }
    let suffix = if stale.len() == 1 { "flow" } else { "flows" };
    let header = format!(
        "possibly stale: {} {suffix} whose anchored code changed but the flow did not",
        stale.len()
    );
    output.push('\n');
    if let Some(colour) = canvas::mark_sgr(crate::Mark::Changed).filter(|_| opts.color) {
        output.push_str(&format!("{colour}{header}\x1b[0m\n"));
    } else {
        output.push_str(&format!("{header}\n"));
    }
    for entry in stale {
        output.push_str(&format!("  {} ({})\n", entry.flow.title, entry.flow.path));
        for hit in &entry.hits {
            let listed: Vec<_> = hit
                .files
                .iter()
                .take(MAX_STALE_FILES)
                .map(String::as_str)
                .collect();
            output.push_str(&format!(
                "    {}: {}",
                hit.target.label(),
                listed.join(", ")
            ));
            let remaining = hit.files.len().saturating_sub(MAX_STALE_FILES);
            if remaining > 0 {
                output.push_str(&format!(", … (+{remaining} more)"));
            }
            output.push('\n');
        }
    }
    output
}

pub fn render_list(flows: &[Flow]) -> String {
    let width = flows.iter().map(|flow| flow.name.len()).max().unwrap_or(0);
    let mut output = String::new();
    for flow in flows {
        output.push_str(&format!("{:<width$}  {}", flow.name, flow.title));
        if let Some(description) = &flow.description {
            output.push_str(&format!(" — {description}"));
        }
        if flow.source.is_none() {
            output.push_str("  (no mermaid block)");
        }
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_description_and_first_block() {
        let flow = parse_flow(
            "checkout.md",
            "# Checkout\n\nA customer pays.\n\n```mermaid\ngraph TD\na --> b\n```\n```mermaid\ngraph TD\nc --> d\n```",
        );
        assert_eq!(flow.title, "Checkout");
        assert_eq!(flow.description.as_deref(), Some("A customer pays."));
        assert_eq!(flow.line_offset, 5);
        assert_eq!(flow.source.as_deref(), Some("graph TD\na --> b\n"));
        assert_eq!(flow.name, "checkout");
    }

    #[test]
    fn mmd_flow_is_whole_file() {
        let source = "graph TD\na --> b";
        let flow = parse_flow("x.mmd", source);
        assert_eq!(flow.title, "x");
        assert_eq!(flow.source.as_deref(), Some(source));
        assert_eq!(flow.description, None);
        assert_eq!(flow.line_offset, 0);
    }

    #[test]
    fn compare_classifies_flows() {
        let old = load_dir(Path::new("tests/fixtures/flows/before")).unwrap();
        let new = load_dir(Path::new("tests/fixtures/flows/after")).unwrap();
        let changes = compare(&old, &new);
        assert!(
            matches!(&changes[0], FlowChange::Modified { flow, .. } if flow.name == "checkout")
        );
        assert!(
            matches!(&changes[1], FlowChange::Removed { flow } if flow.name == "legacy-export")
        );
        assert!(matches!(&changes[2], FlowChange::Unchanged { flow } if flow.name == "login"));
        assert!(matches!(&changes[3], FlowChange::New { flow, .. } if flow.name == "refund"));
    }

    #[test]
    fn broken_current_flow_is_reported() {
        let current = parse_flow("bad.md", "# Bad\n\n```mermaid\ngraph TD\n a -->\n```");
        let changes = compare(&[], &[current]);
        assert!(matches!(&changes[0], FlowChange::Broken { error, .. } if error.line == 5));
        let report = render_report(&changes, &[], "HEAD", "123456789", &Options::default());
        assert!(
            report.contains("Bad (bad.md)  parse error, line 5:"),
            "{report}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_catalog_reports_dangling_flow_symlink() {
        let directory = assert_fs::TempDir::new().unwrap();
        std::os::unix::fs::symlink("missing", directory.path().join("flow.md")).unwrap();
        let entries = read_catalog(directory.path()).unwrap();
        assert!(
            matches!(&entries[..], [Entry::Unreadable { name, error }] if name == "flow.md" && error.kind() == std::io::ErrorKind::NotFound)
        );
        assert!(load_dir(directory.path()).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn read_catalog_ignores_non_flow_entries_without_stat() {
        use std::os::unix::fs::PermissionsExt;
        let directory = assert_fs::TempDir::new().unwrap();
        let notes = directory.path().join("notes.txt");
        std::fs::write(&notes, "private").unwrap();
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o0)).unwrap();
        std::os::unix::fs::symlink("loop.txt", directory.path().join("loop.txt")).unwrap();
        let result = read_catalog(directory.path());
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o600)).unwrap();
        let entries = result.unwrap();
        assert!(
            matches!(&entries[..], [Entry::Ignored { name: first }, Entry::Ignored { name: second }] if first == "loop.txt" && second == "notes.txt")
        );
    }
}
