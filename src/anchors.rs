use std::path::Path;

use crate::flows::Flow;

/// What an anchor ties to code: one step, or the whole flow (`*`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Flow,
    Step(String),
}

impl Target {
    /// Display name shared by anchor reports and footers.
    pub fn label(&self) -> &str {
        match self {
            Self::Flow => "whole flow",
            Self::Step(id) => id,
        }
    }
}

/// A source-located target with its valid repository-relative patterns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub target: Target,
    pub patterns: Vec<String>,
    pub line: usize,
}

/// An anchor syntax error at a file line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorError {
    pub line: usize,
    pub message: String,
}

pub(crate) fn code_tokens(line: &str) -> Option<std::str::SplitWhitespace<'_>> {
    let mut tokens = line.trim().strip_prefix("%%")?.split_whitespace();
    if tokens.next()? != "@code" {
        return None;
    }
    Some(tokens)
}

/// Anchors in a mermaid block. `offset` is the block's fence line; source line i (0-based) is file line `offset + i + 1`.
pub fn parse_anchors(block_source: &str, offset: usize) -> (Vec<Anchor>, Vec<AnchorError>) {
    let mut anchors = Vec::new();
    let mut errors = Vec::new();
    for (index, source_line) in block_source.lines().enumerate() {
        let Some(mut tokens) = code_tokens(source_line) else {
            continue;
        };
        let line = offset + index + 1;
        let target = tokens.next();
        let raw_patterns: Vec<_> = tokens.collect();
        let Some(target) = target else {
            errors.push(AnchorError {
                line,
                message: "@code needs a target and at least one path".into(),
            });
            continue;
        };
        if raw_patterns.is_empty() {
            errors.push(AnchorError {
                line,
                message: "@code needs a target and at least one path".into(),
            });
            continue;
        }
        let target = if target == "*" {
            Target::Flow
        } else {
            Target::Step(target.into())
        };
        let mut patterns = Vec::new();
        for raw in raw_patterns {
            match normalise_pattern(raw) {
                Ok(pattern) => patterns.push(pattern),
                Err(message) => errors.push(AnchorError { line, message }),
            }
        }
        anchors.push(Anchor {
            target,
            patterns,
            line,
        });
    }
    (anchors, errors)
}

/// The flow's anchors (first block only), via `flow.source` and `flow.line_offset`.
pub fn flow_anchors(flow: &Flow) -> (Vec<Anchor>, Vec<AnchorError>) {
    match &flow.source {
        Some(source) => parse_anchors(source, flow.line_offset),
        None => (Vec::new(), Vec::new()),
    }
}

/// Normalise a raw pattern, or explain why it is rejected.
pub fn normalise_pattern(raw: &str) -> Result<String, String> {
    if raw.contains('\\') {
        return Err(format!("\"{raw}\": use / in paths"));
    }
    let relative_error =
        || format!("\"{raw}\": paths are relative to the repository root and stay inside it");
    if raw.starts_with('/') {
        return Err(relative_error());
    }
    let without_prefix = raw.strip_prefix("./").unwrap_or(raw);
    let segments: Vec<_> = without_prefix
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.is_empty()
        || segments.first().is_some_and(|first| is_drive_prefix(first))
        || segments.iter().any(|segment| invalid_segment(segment))
    {
        return Err(relative_error());
    }
    let mut collapsed = Vec::new();
    for segment in segments {
        if segment == "**" && collapsed.last() == Some(&"**") {
            continue;
        }
        collapsed.push(segment);
    }
    Ok(collapsed.join("/"))
}

fn invalid_segment(segment: &str) -> bool {
    segment == "." || segment == ".."
}

/// A Windows drive such as `C:`; only meaningful as the first segment, since `docs/a:b.md` is a valid name.
fn is_drive_prefix(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// True when `pattern` (already normalised) matches the root-relative `/` path.
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    if !pattern.contains(['*', '?']) {
        return path == pattern
            || path
                .strip_prefix(pattern)
                .is_some_and(|tail| tail.starts_with('/'));
    }
    match_segments(
        &pattern.split('/').collect::<Vec<_>>(),
        &path.split('/').collect::<Vec<_>>(),
    )
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    let mut memo = vec![None; (pattern.len() + 1) * (path.len() + 1)];
    match_segments_at(pattern, path, 0, 0, &mut memo)
}

fn match_segments_at(
    pattern: &[&str],
    path: &[&str],
    pattern_index: usize,
    path_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let memo_index = pattern_index * (path.len() + 1) + path_index;
    if let Some(result) = memo[memo_index] {
        return result;
    }
    let result = match pattern.get(pattern_index) {
        None => path_index == path.len(),
        Some(&"**") => {
            match_segments_at(pattern, path, pattern_index + 1, path_index, memo)
                || (path_index < path.len()
                    && match_segments_at(pattern, path, pattern_index, path_index + 1, memo))
        }
        Some(segment) => {
            path.get(path_index)
                .is_some_and(|name| segment_matches(segment, name))
                && match_segments_at(pattern, path, pattern_index + 1, path_index + 1, memo)
        }
    };
    memo[memo_index] = Some(result);
    result
}

/// Whether any valid anchor pattern covers a root-relative path.
pub fn any_pattern_matches(patterns: &[String], path: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| pattern_matches(pattern, path))
}

pub(crate) fn anchor_step_exists(diagram: &crate::Diagram, target: &Target) -> bool {
    match target {
        Target::Flow => true,
        Target::Step(id) => diagram.nodes.iter().any(|node| node.id == *id),
    }
}

fn segment_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<_> = pattern.chars().collect();
    let name: Vec<_> = name.chars().collect();
    let mut pattern_index = 0;
    let mut name_index = 0;
    let mut last_star = None;
    let mut star_resume_index = 0;
    while name_index < name.len() {
        if pattern.get(pattern_index) == Some(&'*') {
            last_star = Some(pattern_index);
            pattern_index += 1;
            star_resume_index = name_index;
        } else if pattern
            .get(pattern_index)
            .is_some_and(|character| *character == '?' || *character == name[name_index])
        {
            pattern_index += 1;
            name_index += 1;
        } else if let Some(star_index) = last_star {
            star_resume_index += 1;
            name_index = star_resume_index;
            pattern_index = star_index + 1;
        } else {
            return false;
        }
    }
    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}

/// Sorted root-relative files, excluding .git and directory symlink traversal.
pub fn walk_files(root: &Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    walk_directory(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk_directory(root: &Path, directory: &Path, files: &mut Vec<String>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if directory != root && error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if entry.file_name() != ".git" {
                walk_directory(root, &path, files)?;
            }
        } else {
            let relative = path.strip_prefix(root).map_err(std::io::Error::other)?;
            files.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

pub(crate) fn grouped_anchors(flow: &Flow) -> Vec<Anchor> {
    let (anchors, _) = flow_anchors(flow);
    let mut grouped: Vec<Anchor> = Vec::new();
    for anchor in anchors {
        if let Some(existing) = grouped
            .iter_mut()
            .find(|existing| existing.target == anchor.target)
        {
            for pattern in anchor.patterns {
                if !existing.patterns.contains(&pattern) {
                    existing.patterns.push(pattern);
                }
            }
        } else {
            grouped.push(anchor);
        }
    }
    grouped
}

/// Code anchors grouped by target in first-appearance order.
pub fn render_footer(flow: &Flow) -> String {
    let anchors = grouped_anchors(flow);
    if anchors.is_empty() {
        return String::new();
    }
    let mut output = String::from("\nCode:\n");
    for anchor in anchors {
        output.push_str(&format!(
            "  {}: {}\n",
            anchor.target.label(),
            anchor.patterns.join(", ")
        ));
    }
    output
}
