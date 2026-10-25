use std::path::{Path, PathBuf};

use crate::diff::{self, DiagramDiff};
use crate::render::bold_line;
use crate::{Diagram, Options, ParseError, canvas, markdown, parser, paths};

pub const SHORT_SHA_LEN: usize = 7;

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
    if paths::has_extension(Path::new(path), &["mmd"]) {
        return Flow {
            path: path.to_string(),
            title: name.clone(),
            name,
            description: None,
            source: Some(text.to_string()),
            line_offset: 0,
        };
    }
    let block = markdown::mermaid_blocks(text).into_iter().next();
    Flow {
        path: path.to_string(),
        title: markdown::first_heading(text).unwrap_or_else(|| name.clone()),
        name,
        description: markdown::first_paragraph_line(text),
        line_offset: block.as_ref().map(|block| block.line).unwrap_or(0),
        source: block.map(|block| block.source),
    }
}

/// Errors include the opening fence offset so their lines refer to the file.
pub fn diagram(flow: &Flow) -> Option<Result<Diagram, ParseError>> {
    flow.source.as_ref().map(|source| {
        parser::parse(source).map_err(|error| ParseError {
            line: flow.line_offset + error.line,
            message: error.message,
        })
    })
}

pub fn is_flow_file_name(name: &str) -> bool {
    !name.starts_with('.') && paths::has_extension(Path::new(name), &["md", "mmd"])
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

pub fn load_dir(dir: &Path) -> Result<Vec<Flow>, CatalogError> {
    let mut flows = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|error| file_error(dir, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| file_error(dir, error))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_flow_file_name(&name) {
            continue;
        }
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => continue,
            // A dangling symlink, or a file removed while scanning, is not a flow.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(file_error(&path, error)),
        }
        let text = std::fs::read_to_string(&path).map_err(|error| file_error(&path, error))?;
        flows.push(parse_flow(&name, &text));
    }
    flows.sort_by(|left, right| left.path.cmp(&right.path));
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

pub fn render_report(changes: &[FlowChange], since: &str, base: &str, opts: &Options) -> String {
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
        let report = render_report(&changes, "HEAD", "123456789", &Options::default());
        assert!(
            report.contains("Bad (bad.md)  parse error, line 5:"),
            "{report}"
        );
    }
}
