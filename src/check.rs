use std::collections::BTreeMap;
use std::path::Path;

use unicode_width::UnicodeWidthStr;

use crate::flows::{CatalogError, Entry};
use crate::{Diagram, anchors, flows, markdown, model::Shape, parser};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    MissingTitle,
    MissingDescription,
    MissingDiagram,
    NotAFlowchart,
    ParseError,
    EmptyDiagram,
    UnclosedFence,
    DuplicateName,
    Unreadable,
    ExtraDiagram,
    DuplicateTitle,
    EmptyCatalog,
    IgnoredEntry,
    MmdFlow,
    NoActor,
    UnlabelledBranch,
    UnlabelledStep,
    TooManySteps,
    TooFewSteps,
    Styling,
    FileName,
    LongLabel,
    EdgeIntoActor,
    AnchorSyntax,
    AnchorUnknownStep,
    AnchorNoMatch,
}

impl Code {
    /// Must list every variant exactly once; the exhaustive test mapping enforces this.
    pub const ALL: [Code; 26] = [
        Self::MissingTitle,
        Self::MissingDescription,
        Self::MissingDiagram,
        Self::NotAFlowchart,
        Self::ParseError,
        Self::EmptyDiagram,
        Self::UnclosedFence,
        Self::DuplicateName,
        Self::Unreadable,
        Self::ExtraDiagram,
        Self::DuplicateTitle,
        Self::EmptyCatalog,
        Self::IgnoredEntry,
        Self::MmdFlow,
        Self::NoActor,
        Self::UnlabelledBranch,
        Self::UnlabelledStep,
        Self::TooManySteps,
        Self::TooFewSteps,
        Self::Styling,
        Self::FileName,
        Self::LongLabel,
        Self::EdgeIntoActor,
        Self::AnchorSyntax,
        Self::AnchorUnknownStep,
        Self::AnchorNoMatch,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingTitle => "missing-title",
            Self::MissingDescription => "missing-description",
            Self::MissingDiagram => "missing-diagram",
            Self::NotAFlowchart => "not-a-flowchart",
            Self::ParseError => "parse-error",
            Self::EmptyDiagram => "empty-diagram",
            Self::UnclosedFence => "unclosed-fence",
            Self::DuplicateName => "duplicate-name",
            Self::Unreadable => "unreadable",
            Self::ExtraDiagram => "extra-diagram",
            Self::DuplicateTitle => "duplicate-title",
            Self::EmptyCatalog => "empty-catalog",
            Self::IgnoredEntry => "ignored-entry",
            Self::MmdFlow => "mmd-flow",
            Self::NoActor => "no-actor",
            Self::UnlabelledBranch => "unlabelled-branch",
            Self::UnlabelledStep => "unlabelled-step",
            Self::TooManySteps => "too-many-steps",
            Self::TooFewSteps => "too-few-steps",
            Self::Styling => "styling",
            Self::FileName => "file-name",
            Self::LongLabel => "long-label",
            Self::EdgeIntoActor => "edge-into-actor",
            Self::AnchorSyntax => "anchor-syntax",
            Self::AnchorUnknownStep => "anchor-unknown-step",
            Self::AnchorNoMatch => "anchor-no-match",
        }
    }

    pub fn severity(self) -> Severity {
        match self {
            Self::MissingTitle
            | Self::MissingDescription
            | Self::MissingDiagram
            | Self::NotAFlowchart
            | Self::ParseError
            | Self::EmptyDiagram
            | Self::UnclosedFence
            | Self::DuplicateName
            | Self::Unreadable
            | Self::AnchorSyntax
            | Self::AnchorUnknownStep
            | Self::AnchorNoMatch => Severity::Error,
            Self::ExtraDiagram
            | Self::DuplicateTitle
            | Self::EmptyCatalog
            | Self::IgnoredEntry
            | Self::MmdFlow
            | Self::NoActor
            | Self::UnlabelledBranch
            | Self::UnlabelledStep
            | Self::TooManySteps
            | Self::TooFewSteps
            | Self::Styling
            | Self::FileName
            | Self::LongLabel
            | Self::EdgeIntoActor => Severity::Warning,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub code: Code,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub flows: usize,
    pub problems: Vec<Problem>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.problems
            .iter()
            .filter(|problem| problem.code.severity() == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.problems
            .iter()
            .filter(|problem| problem.code.severity() == Severity::Warning)
            .count()
    }
}

pub const MIN_STEPS: usize = 3;
pub const MAX_STEPS: usize = 15;
pub const MAX_LABEL_WIDTH: usize = 40;
const LABEL_EXCERPT_CHARS: usize = 20;

/// Returns problems sorted by line and code within the file.
pub fn check_flow(file: &str, text: &str) -> Vec<Problem> {
    check_file(file, text, &mut |_| true).1
}

fn check_file(
    file: &str,
    text: &str,
    has_match: &mut dyn FnMut(&str) -> bool,
) -> (Option<String>, Vec<Problem>) {
    let mut problems = Vec::new();
    let stem = Path::new(file)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    if !is_kebab_case(&stem) {
        problems.push(problem(
            file,
            None,
            Code::FileName,
            "name flow files in kebab-case",
        ));
    }
    let title = if flows::is_mmd(file) {
        check_mmd_flow(file, text, &mut problems, has_match);
        None
    } else {
        check_markdown_flow(file, text, &mut problems, has_match)
    };
    sort_problems(&mut problems);
    (title, problems)
}

fn check_mmd_flow(
    file: &str,
    text: &str,
    problems: &mut Vec<Problem>,
    has_match: &mut dyn FnMut(&str) -> bool,
) {
    problems.push(problem(
        file,
        None,
        Code::MmdFlow,
        "use a .md file with a title and description",
    ));
    check_diagram(file, &crate::skill::normalise(text), 0, problems, has_match);
}

fn check_markdown_flow(
    file: &str,
    text: &str,
    problems: &mut Vec<Problem>,
    has_match: &mut dyn FnMut(&str) -> bool,
) -> Option<String> {
    let outline = markdown::outline(text);
    problems.extend(check_title_and_description(file, &outline));
    check_unclosed_fences(file, &outline, problems);
    check_misplaced_anchors(file, text, &outline, problems);
    check_diagrams(file, &outline, problems, has_match);
    outline.title.map(|(_, title)| title)
}

fn check_title_and_description(file: &str, outline: &markdown::Outline) -> Vec<Problem> {
    if outline.title.is_none() {
        vec![problem(
            file,
            Some(1),
            Code::MissingTitle,
            "no \"# Title\" heading (setext and \"#Title\" are not recognised)",
        )]
    } else if let Some((line, _)) = &outline.title
        && outline.description.is_none()
    {
        vec![problem(
            file,
            Some(*line),
            Code::MissingDescription,
            "no one-sentence description after the title",
        )]
    } else {
        Vec::new()
    }
}

fn check_unclosed_fences(file: &str, outline: &markdown::Outline, problems: &mut Vec<Problem>) {
    for block in &outline.blocks {
        if !block.closed {
            problems.push(problem(
                file,
                Some(block.line),
                Code::UnclosedFence,
                "code fence is never closed",
            ));
        }
    }
}

fn check_diagrams(
    file: &str,
    outline: &markdown::Outline,
    problems: &mut Vec<Problem>,
    has_match: &mut dyn FnMut(&str) -> bool,
) {
    let mut diagrams = outline
        .blocks
        .iter()
        .filter(|block| markdown::is_mermaid(&block.info));
    if let Some(first) = diagrams.next() {
        check_diagram(file, &first.source, first.line, problems, has_match);
        for extra in diagrams {
            problems.push(problem(
                file,
                Some(extra.line),
                Code::ExtraDiagram,
                "only the first mermaid block is the flow; move this one to its own file",
            ));
        }
    } else {
        problems.push(problem(
            file,
            Some(1),
            Code::MissingDiagram,
            "no mermaid flowchart block",
        ));
    }
}

fn check_misplaced_anchors(
    file: &str,
    text: &str,
    outline: &markdown::Outline,
    problems: &mut Vec<Problem>,
) {
    let first = outline
        .blocks
        .iter()
        .find(|block| markdown::is_mermaid(&block.info));
    for (index, line) in crate::skill::normalise(text).lines().enumerate() {
        let line_number = index + 1;
        let inside_first = first.is_some_and(|block| {
            line_number > block.line && line_number <= block.line + block.source.lines().count()
        });
        if !inside_first && anchors::code_tokens(line).is_some() {
            problems.push(problem(
                file,
                Some(line_number),
                Code::AnchorSyntax,
                "anchors belong in the flow's first mermaid block",
            ));
        }
    }
}

fn check_anchor_paths(
    file: &str,
    anchors: &[anchors::Anchor],
    has_match: &mut dyn FnMut(&str) -> bool,
    problems: &mut Vec<Problem>,
) {
    for anchor in anchors {
        for pattern in &anchor.patterns {
            if !has_match(pattern) {
                problems.push(problem(file, Some(anchor.line), Code::AnchorNoMatch, format!("\"{pattern}\" matches no file in the repository; the code moved or the anchor is wrong")));
            }
        }
    }
}

fn check_anchor_steps(
    file: &str,
    anchors: &[anchors::Anchor],
    diagram: &Diagram,
    problems: &mut Vec<Problem>,
) {
    for anchor in anchors {
        if let anchors::Target::Step(id) = &anchor.target
            && !diagram.nodes.iter().any(|node| node.id == *id)
        {
            problems.push(problem(
                file,
                Some(anchor.line),
                Code::AnchorUnknownStep,
                format!("@code points at step \"{id}\", which this flowchart does not have"),
            ));
        }
    }
}

fn problem(file: &str, line: Option<usize>, code: Code, message: impl Into<String>) -> Problem {
    Problem {
        file: Some(file.to_string()),
        line,
        code,
        message: message.into(),
    }
}

fn is_kebab_case(name: &str) -> bool {
    name.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// A following `-`, `=` or `&` makes the keyword a linked node id rather than a styling statement.
fn is_styling(line: &str) -> bool {
    let trimmed = line.trim_start().to_ascii_lowercase();
    if line.contains(":::") {
        return true;
    }
    let Some((keyword, rest)) = trimmed.split_once(char::is_whitespace) else {
        return trimmed.starts_with("classdef") || trimmed.starts_with("linkstyle");
    };
    if rest.trim_start().starts_with(['-', '=', '&']) {
        return false;
    }
    matches!(
        keyword,
        "classdef" | "class" | "style" | "linkstyle" | "click"
    )
}

fn check_diagram(
    file: &str,
    source: &str,
    offset: usize,
    problems: &mut Vec<Problem>,
    has_match: &mut dyn FnMut(&str) -> bool,
) {
    let (anchors, errors) = anchors::parse_anchors(source, offset);
    for error in errors {
        problems.push(problem(
            file,
            Some(error.line),
            Code::AnchorSyntax,
            error.message,
        ));
    }
    check_anchor_paths(file, &anchors, has_match, problems);
    let fence_line = offset.max(1);
    for (index, line) in source.lines().enumerate() {
        if line.trim_start().starts_with("%%") {
            continue;
        }
        for statement in parser::split_statements(line) {
            if is_styling(&statement) {
                problems.push(problem(
                    file,
                    Some(offset + index + 1),
                    Code::Styling,
                    "styling is ignored by strata; remove it",
                ));
            }
        }
    }
    let first = parser::first_statement(source);
    if !first.is_some_and(|(_, statement)| parser::is_header(statement)) {
        problems.push(problem(
            file,
            Some(offset + first.map_or(1, |(line, _)| line)),
            Code::NotAFlowchart,
            "the first mermaid block must start with \"flowchart TD\" (or graph/LR/…)",
        ));
        return;
    }
    match flows::parse_diagram(source, offset) {
        Ok(diagram) => {
            check_anchor_steps(file, &anchors, &diagram, problems);
            if diagram.nodes.is_empty() {
                problems.push(problem(
                    file,
                    Some(fence_line),
                    Code::EmptyDiagram,
                    "the flowchart has no steps",
                ));
            }
            check_actor(file, fence_line, &diagram, problems);
            check_nodes(file, source, offset, &diagram, problems);
            check_edges_into_actor(file, source, offset, &diagram, problems);
            check_step_count(file, fence_line, &diagram, problems);
        }
        Err(error) => problems.push(problem(
            file,
            Some(error.line),
            Code::ParseError,
            error.message,
        )),
    }
}

fn has_unlabelled_exit(diagram: &Diagram, node_index: usize) -> bool {
    diagram.edges.iter().any(|edge| {
        let outgoing = edge.from == node_index || (edge.arrow_from && edge.to == node_index);
        outgoing
            && edge
                .label
                .as_ref()
                .is_none_or(|label| label.trim().is_empty())
    })
}

fn check_actor(file: &str, line: usize, diagram: &Diagram, problems: &mut Vec<Problem>) {
    if !diagram
        .nodes
        .iter()
        .any(|node| node.shape == Shape::Stadium)
    {
        problems.push(problem(
            file,
            Some(line),
            Code::NoActor,
            "start the flow from the actor as a stadium ([...])",
        ));
    }
}

fn node_line(block_source: &str, id: &str) -> Option<usize> {
    for (index, line) in block_source.lines().enumerate() {
        if line.trim_start().starts_with("%%") {
            continue;
        }
        for statement in parser::split_statements(line) {
            if parser::statement_mentions_id(&statement, id) {
                return Some(index + 1);
            }
        }
    }
    None
}

fn node_file_line(source: &str, offset: usize, id: &str) -> usize {
    node_line(source, id).map_or(offset.max(1), |line| offset + line)
}

fn label_excerpt(label: &str) -> String {
    let mut characters = label.chars();
    let mut excerpt: String = characters.by_ref().take(LABEL_EXCERPT_CHARS).collect();
    if characters.next().is_some() {
        excerpt.push('…');
    }
    excerpt
}

fn check_nodes(
    file: &str,
    source: &str,
    offset: usize,
    diagram: &Diagram,
    problems: &mut Vec<Problem>,
) {
    for (index, node) in diagram.nodes.iter().enumerate() {
        let line = node_file_line(source, offset, &node.id);
        if node.shape == Shape::Diamond && has_unlabelled_exit(diagram, index) {
            problems.push(problem(
                file,
                Some(line),
                Code::UnlabelledBranch,
                format!("label every branch of decision \"{}\"", node.id),
            ));
        }
        if node
            .label
            .iter()
            .any(|label| label.width() > MAX_LABEL_WIDTH)
        {
            let label = node.label.join(" ");
            let excerpt = label_excerpt(&label);
            problems.push(problem(file, Some(line), Code::LongLabel,
                format!("step \"{}\" (\"{excerpt}\") is wider than {MAX_LABEL_WIDTH} columns; shorten it so the flow fits a side pane", node.id)));
        }
        if node.label.as_slice() == [node.id.as_str()] {
            problems.push(problem(
                file,
                Some(line),
                Code::UnlabelledStep,
                format!("step \"{}\" has no label", node.id),
            ));
        }
    }
}

fn check_edges_into_actor(
    file: &str,
    source: &str,
    offset: usize,
    diagram: &Diagram,
    problems: &mut Vec<Problem>,
) {
    for edge in &diagram.edges {
        let target = &diagram.nodes[edge.to];
        let origin = &diagram.nodes[edge.from];
        let mut actors = Vec::new();
        if target.shape == Shape::Stadium {
            actors.push(target);
        }
        if edge.arrow_from && origin.shape == Shape::Stadium {
            actors.push(origin);
        }
        for actor in actors {
            let line = node_file_line(source, offset, &actor.id);
            problems.push(problem(file, Some(line), Code::EdgeIntoActor,
                format!("edge into the actor \"{}\"; end the branch or loop back to the step the user retries", actor.id)));
        }
    }
}

fn check_step_count(file: &str, line: usize, diagram: &Diagram, problems: &mut Vec<Problem>) {
    if diagram.nodes.len() > MAX_STEPS {
        problems.push(problem(
            file,
            Some(line),
            Code::TooManySteps,
            format!("more than {MAX_STEPS} steps; split it into separate use cases"),
        ));
    } else if !diagram.nodes.is_empty() && diagram.nodes.len() < MIN_STEPS {
        problems.push(problem(
            file,
            Some(line),
            Code::TooFewSteps,
            format!("fewer than {MIN_STEPS} steps"),
        ));
    }
}

pub fn check_catalog(dir: &Path) -> Result<Report, CatalogError> {
    check_catalog_with(dir, &mut |_| true)
}

/// `has_match` is called once per valid anchor pattern; false reports anchor-no-match.
pub fn check_catalog_with(
    dir: &Path,
    has_match: &mut dyn FnMut(&str) -> bool,
) -> Result<Report, CatalogError> {
    let mut report = Report {
        flows: 0,
        problems: Vec::new(),
    };
    let mut names = BTreeMap::new();
    let mut titles = BTreeMap::new();
    for entry in flows::read_catalog(dir)? {
        match entry {
            Entry::Flow { name, text } => {
                report.flows += 1;
                check_duplicate_name(&name, &mut names, &mut report.problems);
                let (title, problems) = check_file(&name, &text, has_match);
                if let Some(title) = title {
                    record_title(&name, &title, &mut titles, &mut report.problems);
                }
                report.problems.extend(problems);
            }
            Entry::Unreadable { name, error } => {
                report.flows += 1;
                check_duplicate_name(&name, &mut names, &mut report.problems);
                report.problems.push(problem(
                    &name,
                    None,
                    Code::Unreadable,
                    format!("cannot read: {error}"),
                ));
            }
            Entry::Ignored { name } => report.problems.push(problem(
                &name,
                None,
                Code::IgnoredEntry,
                "not a flow file (flows are top-level .md files)",
            )),
        }
    }
    if report.flows == 0 {
        report.problems.push(Problem {
            file: None,
            line: None,
            code: Code::EmptyCatalog,
            message: "no flows yet".to_string(),
        });
    }
    sort_problems(&mut report.problems);
    Ok(report)
}

fn record_title(
    file: &str,
    title: &str,
    titles: &mut BTreeMap<String, String>,
    problems: &mut Vec<Problem>,
) {
    let title = title.trim().to_lowercase();
    if let Some(first) = titles.get(&title) {
        problems.push(problem(
            file,
            None,
            Code::DuplicateTitle,
            format!("same title as {first}"),
        ));
    } else {
        titles.insert(title, file.to_string());
    }
}

fn check_duplicate_name(
    file: &str,
    names: &mut BTreeMap<String, String>,
    problems: &mut Vec<Problem>,
) {
    let name = Path::new(file)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if let Some(first) = names.get(&name) {
        problems.push(problem(
            file,
            None,
            Code::DuplicateName,
            format!("same flow name as {first}"),
        ));
    } else {
        names.insert(name, file.to_string());
    }
}

fn sort_problems(problems: &mut [Problem]) {
    problems.sort_by(|left, right| {
        (&left.file, left.line, left.code).cmp(&(&right.file, right.line, right.code))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill;

    const VALID: &str = "# Checkout\n\nA customer checks out.\n\n```mermaid\nflowchart TD\nactor([Customer]) --> pay[Pay]\npay --> ok{Accepted?}\nok -->|yes| done[Done]\nok -->|no| pay\n```\n";

    fn catalog_problems(files: &[(&str, &[u8])], directories: &[&str]) -> Vec<Problem> {
        let directory = assert_fs::TempDir::new().unwrap();
        for (name, contents) in files {
            std::fs::write(directory.path().join(name), contents).unwrap();
        }
        for name in directories {
            std::fs::create_dir(directory.path().join(name)).unwrap();
        }
        check_catalog(directory.path()).unwrap().problems
    }

    struct Case {
        code: Code,
        severity: Severity,
        file: &'static str,
        line: Option<usize>,
        problems: Vec<Problem>,
        expected_codes: Option<&'static [Code]>,
    }

    fn single_flow_case(
        code: Code,
        file: &'static str,
        line: Option<usize>,
        text: &str,
        expected_codes: &'static [Code],
    ) -> Case {
        Case {
            code,
            severity: expected_severity(code),
            file,
            line,
            problems: check_flow(file, text),
            expected_codes: Some(expected_codes),
        }
    }

    fn expected_severity(code: Code) -> Severity {
        match code {
            Code::MissingTitle
            | Code::MissingDescription
            | Code::MissingDiagram
            | Code::NotAFlowchart
            | Code::ParseError
            | Code::EmptyDiagram
            | Code::UnclosedFence
            | Code::DuplicateName
            | Code::Unreadable
            | Code::AnchorSyntax
            | Code::AnchorUnknownStep
            | Code::AnchorNoMatch => Severity::Error,
            Code::ExtraDiagram
            | Code::DuplicateTitle
            | Code::EmptyCatalog
            | Code::IgnoredEntry
            | Code::MmdFlow
            | Code::NoActor
            | Code::UnlabelledBranch
            | Code::UnlabelledStep
            | Code::TooManySteps
            | Code::TooFewSteps
            | Code::Styling
            | Code::FileName
            | Code::LongLabel
            | Code::EdgeIntoActor => Severity::Warning,
        }
    }

    fn cases() -> Vec<Case> {
        vec![
            // Without a recognised title or a fence, both required elements are missing.
            single_flow_case(
                Code::MissingTitle,
                "checkout.md",
                Some(1),
                "#Title\n",
                &[Code::MissingTitle, Code::MissingDiagram],
            ),
            single_flow_case(
                Code::MissingDescription,
                "checkout.md",
                Some(1),
                &VALID.replace("\nA customer checks out.\n", "\n"),
                &[Code::MissingDescription],
            ),
            single_flow_case(
                Code::MissingDiagram,
                "checkout.md",
                Some(1),
                "# Checkout\nDescription.\n",
                &[Code::MissingDiagram],
            ),
            single_flow_case(
                Code::NotAFlowchart,
                "checkout.md",
                Some(6),
                &VALID.replace("flowchart TD", "sequenceDiagram"),
                &[Code::NotAFlowchart],
            ),
            single_flow_case(
                Code::ParseError,
                "checkout.md",
                Some(7),
                &VALID.replace("actor([Customer]) --> pay[Pay]", "actor -->"),
                &[Code::ParseError],
            ),
            // An empty diagram necessarily has no actor, so both diagnostics are intended.
            single_flow_case(
                Code::EmptyDiagram,
                "checkout.md",
                Some(3),
                "# Checkout\nDescription.\n```mermaid\nflowchart TD\n```\n",
                &[Code::EmptyDiagram, Code::NoActor],
            ),
            // This unclosed non-Mermaid fence also leaves the document without a diagram.
            single_flow_case(
                Code::UnclosedFence,
                "checkout.md",
                Some(3),
                "# Checkout\nDescription.\n```rust\ncode\n",
                &[Code::MissingDiagram, Code::UnclosedFence],
            ),
            Case {
                code: Code::DuplicateName,
                severity: Severity::Error,
                file: "checkout.mmd",
                line: None,
                problems: catalog_problems(
                    &[
                        ("checkout.md", VALID.as_bytes()),
                        ("checkout.mmd", b"flowchart TD"),
                    ],
                    &[],
                ),
                expected_codes: None,
            },
            Case {
                code: Code::Unreadable,
                severity: Severity::Error,
                file: "broken.md",
                line: None,
                problems: catalog_problems(&[("broken.md", &[0xff])], &[]),
                expected_codes: None,
            },
            single_flow_case(
                Code::ExtraDiagram,
                "checkout.md",
                Some(12),
                &format!("{VALID}```mermaid\nflowchart TD\n```\n"),
                &[Code::ExtraDiagram],
            ),
            Case {
                code: Code::DuplicateTitle,
                severity: Severity::Warning,
                file: "b.md",
                line: None,
                problems: catalog_problems(
                    &[
                        ("a.md", VALID.as_bytes()),
                        (
                            "b.md",
                            VALID.replace("# Checkout", "# CHECKOUT ").as_bytes(),
                        ),
                    ],
                    &[],
                ),
                expected_codes: None,
            },
            Case {
                code: Code::EmptyCatalog,
                severity: Severity::Warning,
                file: "",
                line: None,
                problems: catalog_problems(&[], &[]),
                expected_codes: None,
            },
            Case {
                code: Code::IgnoredEntry,
                severity: Severity::Warning,
                file: "notes.txt",
                line: None,
                problems: catalog_problems(&[("notes.txt", b"notes")], &["nested"]),
                expected_codes: None,
            },
            single_flow_case(
                Code::MmdFlow,
                "checkout.mmd",
                None,
                &markdown::mermaid_blocks(VALID)[0].source,
                &[Code::MmdFlow],
            ),
            single_flow_case(
                Code::NoActor,
                "checkout.md",
                Some(5),
                &VALID.replace("actor([Customer])", "actor[Customer]"),
                &[Code::NoActor],
            ),
            single_flow_case(
                Code::UnlabelledBranch,
                "checkout.md",
                Some(8),
                &VALID.replace("-->|yes|", "-->"),
                &[Code::UnlabelledBranch],
            ),
            single_flow_case(
                Code::UnlabelledStep,
                "checkout.md",
                Some(9),
                &VALID.replace("done[Done]", "done"),
                &[Code::UnlabelledStep],
            ),
            single_flow_case(
                Code::TooManySteps,
                "checkout.md",
                Some(3),
                &flowchart_with_steps(MAX_STEPS + 1),
                &[Code::TooManySteps],
            ),
            single_flow_case(
                Code::TooFewSteps,
                "checkout.md",
                Some(3),
                &flowchart_with_steps(MIN_STEPS - 1),
                &[Code::TooFewSteps],
            ),
            single_flow_case(
                Code::Styling,
                "checkout.md",
                Some(11),
                &VALID.replace("```\n", "classDef actor fill:red\n```\n"),
                &[Code::Styling],
            ),
            single_flow_case(
                Code::FileName,
                "Check_out.md",
                None,
                VALID,
                &[Code::FileName],
            ),
            single_flow_case(
                Code::LongLabel,
                "checkout.md",
                Some(7),
                &VALID.replace(
                    "pay[Pay]",
                    &format!("pay[{}]", "x".repeat(MAX_LABEL_WIDTH + 1)),
                ),
                &[Code::LongLabel],
            ),
            single_flow_case(
                Code::EdgeIntoActor,
                "checkout.md",
                Some(7),
                &VALID.replace("```\n", "done --> actor\n```\n"),
                &[Code::EdgeIntoActor],
            ),
            single_flow_case(
                Code::AnchorSyntax,
                "checkout.md",
                Some(11),
                &VALID.replace("```\n", "%% @code pay\n```\n"),
                &[Code::AnchorSyntax],
            ),
            single_flow_case(
                Code::AnchorUnknownStep,
                "checkout.md",
                Some(11),
                &VALID.replace("```\n", "%% @code missing src/pay.rs\n```\n"),
                &[Code::AnchorUnknownStep],
            ),
            anchor_no_match_case(),
        ]
    }

    fn anchor_no_match_case() -> Case {
        let directory = assert_fs::TempDir::new().unwrap();
        let text = VALID.replace("```\n", "%% @code pay src/gone.rs src/pay.rs\n```\n");
        std::fs::write(directory.path().join("checkout.md"), text).unwrap();
        Case {
            code: Code::AnchorNoMatch,
            severity: Severity::Error,
            file: "checkout.md",
            line: Some(11),
            problems: check_catalog_with(directory.path(), &mut |pattern| pattern != "src/gone.rs")
                .unwrap()
                .problems,
            expected_codes: Some(&[Code::AnchorNoMatch]),
        }
    }

    fn position(code: Code) -> usize {
        match code {
            Code::MissingTitle => 0,
            Code::MissingDescription => 1,
            Code::MissingDiagram => 2,
            Code::NotAFlowchart => 3,
            Code::ParseError => 4,
            Code::EmptyDiagram => 5,
            Code::UnclosedFence => 6,
            Code::DuplicateName => 7,
            Code::Unreadable => 8,
            Code::ExtraDiagram => 9,
            Code::DuplicateTitle => 10,
            Code::EmptyCatalog => 11,
            Code::IgnoredEntry => 12,
            Code::MmdFlow => 13,
            Code::NoActor => 14,
            Code::UnlabelledBranch => 15,
            Code::UnlabelledStep => 16,
            Code::TooManySteps => 17,
            Code::TooFewSteps => 18,
            Code::Styling => 19,
            Code::FileName => 20,
            Code::LongLabel => 21,
            Code::EdgeIntoActor => 22,
            Code::AnchorSyntax => 23,
            Code::AnchorUnknownStep => 24,
            Code::AnchorNoMatch => 25,
        }
    }

    fn own_problem(case: &Case) -> &Problem {
        case.problems
            .iter()
            .find(|problem| {
                problem.code == case.code
                    && problem.file.as_deref().unwrap_or_default() == case.file
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing {:?} on {:?}: {:?}",
                    case.code, case.file, case.problems
                )
            })
    }

    #[test]
    fn check_reports_each_problem_code() {
        for case in cases() {
            let problem = own_problem(&case);
            assert_eq!(problem.code.severity(), case.severity, "{:?}", case.code);
            assert_eq!(problem.line, case.line, "{:?}", case.code);
            if let Some(expected_codes) = case.expected_codes {
                let codes: Vec<_> = case.problems.iter().map(|problem| problem.code).collect();
                assert_eq!(codes, expected_codes, "{:?}", case.code);
            }
        }
    }

    #[test]
    fn every_code_is_covered() {
        let cases = cases();
        assert_eq!(cases.len(), Code::ALL.len());
        let mut seen = std::collections::BTreeSet::new();
        for case in cases {
            assert!(seen.insert(case.code), "duplicate case: {:?}", case.code);
            assert_eq!(Code::ALL[position(case.code)], case.code);
            assert_eq!(own_problem(&case).code, case.code);
        }
        for code in Code::ALL {
            assert_eq!(Code::ALL[position(code)], code);
            assert!(seen.contains(&code), "missing dedicated case: {code:?}");
        }
    }

    fn example(marker: &str) -> &str {
        skill::SKILL_MD
            .split_once(marker)
            .unwrap()
            .1
            .split_once("````markdown\n")
            .unwrap()
            .1
            .split_once("````\n<!-- end-example -->")
            .unwrap()
            .0
    }

    #[test]
    fn check_accepts_the_skill_examples() {
        assert_eq!(
            check_flow("check-out-a-cart.md", example("<!-- good-example -->")),
            []
        );
        let problems = check_flow("checkout.md", example("<!-- bad-example codes:"));
        let mut codes: Vec<_> = problems
            .iter()
            .map(|problem| problem.code.as_str())
            .collect();
        codes.sort_unstable();
        codes.dedup();
        let declared = skill::SKILL_MD
            .split_once("<!-- bad-example codes: ")
            .unwrap()
            .1
            .split_once(" -->")
            .unwrap()
            .0;
        assert_eq!(codes.join(" "), declared);
    }

    #[test]
    fn check_skips_shape_warnings_on_parse_errors() {
        let problems = check_flow(
            "checkout.md",
            &VALID.replace("actor([Customer]) --> pay[Pay]", "actor -->"),
        );
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, Code::ParseError);
        assert_eq!(problems[0].message, "link has no target node");
    }

    #[test]
    fn check_empty_diagrams_still_report_a_missing_actor() {
        let problems = check_flow(
            "checkout.md",
            "# Checkout\nDescription.\n```mermaid\nflowchart TD\n```\n",
        );
        let codes: Vec<_> = problems.iter().map(|problem| problem.code).collect();
        assert_eq!(codes, [Code::EmptyDiagram, Code::NoActor]);
    }

    #[test]
    fn check_does_not_treat_fallback_names_as_titles() {
        let problems = catalog_problems(
            &[
                ("checkout.md", b"No heading"),
                ("checkout.mmd", b"flowchart TD"),
            ],
            &[],
        );
        assert!(
            problems
                .iter()
                .any(|problem| problem.code == Code::DuplicateName)
        );
        assert!(
            !problems
                .iter()
                .any(|problem| problem.code == Code::DuplicateTitle)
        );
    }

    #[test]
    fn check_covers_styling_and_mmd_lines() {
        for style in [
            "classDef x fill:red",
            "class actor x",
            "style actor fill:red",
            "linkStyle 0 stroke:red",
            "click actor target",
            "actor:::x",
        ] {
            let text = VALID.replace("```\n", &format!("{style}\n```\n"));
            let problems = check_flow("checkout.md", &text);
            assert_eq!(
                problems
                    .iter()
                    .filter(|problem| problem.code == Code::Styling)
                    .count(),
                1,
                "{style}"
            );
        }
        let problems = check_flow("checkout.mmd", "flowchart TD\nactor -->");
        assert_eq!(
            problems
                .iter()
                .find(|problem| problem.code == Code::ParseError)
                .unwrap()
                .line,
            Some(2)
        );
        assert!(!problems.iter().any(|problem| matches!(
            problem.code,
            Code::MissingTitle | Code::MissingDescription | Code::MissingDiagram
        )));
    }

    #[test]
    fn not_a_flowchart_ignores_mermaid_comments() {
        for comment in ["%% x", "%%{init: {}}%%"] {
            let text = VALID.replace("flowchart TD", &format!("{comment}\nflowchart TD"));
            assert_eq!(check_flow("checkout.md", &text), []);
            let invalid = text.replace("flowchart TD", "sequenceDiagram");
            let problems = check_flow("checkout.md", &invalid);
            assert_eq!(problems[0].code, Code::NotAFlowchart);
            assert_eq!(problems[0].line, Some(7));
        }
    }

    #[test]
    fn check_never_errors_on_renderable_flowcharts() {
        let fixtures = [
            include_str!("../examples/simple.mmd"),
            include_str!("../examples/architecture.mmd"),
            include_str!("../tests/fixtures/fanout.mmd"),
            include_str!("../tests/fixtures/nested.mmd"),
            include_str!("../tests/fixtures/edgecases.mmd"),
            include_str!("../tests/fixtures/pipeline.mmd"),
        ];
        for source in fixtures {
            for prefix in ["", "%% comment\n", "%%{init: {}}%%\n"] {
                let source = format!("{prefix}{source}");
                assert!(crate::render_str(&source, &crate::Options::default()).is_ok());
                let text = format!("# Fixture\nDescription.\n```mermaid\n{source}\n```\n");
                let errors: Vec<_> = check_flow("fixture.md", &text)
                    .into_iter()
                    .filter(|problem| problem.code.severity() == Severity::Error)
                    .collect();
                assert_eq!(errors, [], "{source}");
            }
        }
    }

    fn flowchart_with_steps(count: usize) -> String {
        let mut source = String::from("# Steps\nDescription.\n```mermaid\nflowchart TD\n");
        if count > 0 {
            source.push_str("actor([Customer])\n");
        }
        for index in 1..count {
            source.push_str(&format!("step{index}[Step {index}]\n"));
        }
        source.push_str("```\n");
        source
    }

    #[test]
    fn step_boundaries() {
        for (count, expected) in [
            (MIN_STEPS, vec![]),
            (MIN_STEPS - 1, vec![Code::TooFewSteps]),
            (MAX_STEPS, vec![]),
            (MAX_STEPS + 1, vec![Code::TooManySteps]),
        ] {
            let codes: Vec<_> = check_flow("steps.md", &flowchart_with_steps(count))
                .into_iter()
                .map(|problem| problem.code)
                .collect();
            assert_eq!(codes, expected, "{count}");
        }
    }

    #[test]
    fn label_width_boundaries() {
        for (label, expected) in [
            ("a".repeat(MAX_LABEL_WIDTH), 0),
            ("a".repeat(MAX_LABEL_WIDTH + 1), 1),
            ("界".repeat(MAX_LABEL_WIDTH / 2), 0),
            (format!("{}a", "界".repeat(MAX_LABEL_WIDTH / 2)), 1),
        ] {
            let text = VALID.replace("pay[Pay]", &format!("pay[{label}]"));
            let problems = check_flow("checkout.md", &text);
            assert_eq!(
                problems
                    .iter()
                    .filter(|problem| problem.code.as_str() == "long-label")
                    .count(),
                expected,
                "{label}"
            );
        }
    }

    #[test]
    fn edges_into_actors_are_reported_individually() {
        let text = VALID.replace("```\n", "done --> actor\npay --> actor\n```\n");
        let problems = check_flow("checkout.md", &text);
        assert_eq!(
            problems
                .iter()
                .filter(|problem| problem.code.as_str() == "edge-into-actor")
                .count(),
            2
        );
    }

    #[test]
    fn styling_keywords_can_be_node_ids() {
        for keyword in ["click", "style", "class"] {
            for arrow in ["-->", "==>", "& pay -->"] {
                let text = VALID.replace("```\n", &format!("{keyword} {arrow} done\n```\n"));
                assert!(
                    !check_flow("checkout.md", &text)
                        .iter()
                        .any(|problem| problem.code == Code::Styling),
                    "{keyword} {arrow}"
                );
            }
        }
    }

    #[test]
    fn node_warnings_point_at_the_node_line() {
        let text = "# Nodes\nDescription.\n```mermaid\nflowchart TD\n%% pay --> actor; pay\npayment[Payment]\nactor([Customer]) --> payment; pay\npay --> choice{Choose?}\nchoice --> payment\npayment --> actor\n```\n";
        let warnings: Vec<_> = check_flow("nodes.md", text)
            .into_iter()
            .filter(|problem| {
                matches!(
                    problem.code,
                    Code::UnlabelledStep | Code::UnlabelledBranch | Code::EdgeIntoActor
                )
            })
            .map(|problem| (problem.code, problem.line))
            .collect();
        assert_eq!(
            warnings,
            vec![
                (Code::UnlabelledStep, Some(7)),
                (Code::EdgeIntoActor, Some(7)),
                (Code::UnlabelledBranch, Some(8))
            ]
        );
        let text = VALID.replace("pay[Pay]", &format!("pay[{}]", "界".repeat(21)));
        let warning = check_flow("checkout.md", &text)
            .into_iter()
            .find(|problem| problem.code == Code::LongLabel)
            .unwrap();
        assert_eq!(warning.line, Some(7));
        assert_eq!(
            warning.message,
            format!(
                "step \"pay\" (\"{}…\") is wider than 40 columns; shorten it so the flow fits a side pane",
                "界".repeat(20)
            )
        );
    }

    #[test]
    fn edge_into_actor_catches_bidirectional_links() {
        for link in ["actor <--> pay", "pay <--> actor"] {
            let text = VALID.replace("```\n", &format!("{link}\n```\n"));
            let warnings: Vec<_> = check_flow("checkout.md", &text)
                .into_iter()
                .filter(|problem| problem.code == Code::EdgeIntoActor)
                .collect();
            assert_eq!(warnings.len(), 1, "{link}");
            assert_eq!(warnings[0].line, Some(7));
        }
    }

    #[test]
    fn styling_ignores_comments_and_finds_inline_statements() {
        let text = VALID.replace("```\n", "%% comment ::: ignored; style pay fill:red\npay --> done; style pay fill:red; class pay highlighted\n```\n");
        let lines: Vec<_> = check_flow("checkout.md", &text)
            .into_iter()
            .filter(|problem| problem.code == Code::Styling)
            .map(|problem| problem.line)
            .collect();
        assert_eq!(lines, vec![Some(12), Some(12)]);
    }

    #[test]
    fn three_steps_need_no_padding() {
        assert!(check_flow("steps.md", &flowchart_with_steps(3)).is_empty());
    }

    #[test]
    fn node_lines_match_complete_parser_ids() {
        let source = "flowchart TD\npayment[Payment]; pre-pay[Before payment]\npay.later[Later]\npay-->pay-now[Pay now]\n";
        assert_eq!(node_line(source, "pay"), Some(4));
        assert_eq!(node_line(source, "pre-pay"), Some(2));
        assert_eq!(node_line(source, "pay.later"), Some(3));
        assert_eq!(node_line(source, "pay-now"), Some(4));
        assert_eq!(node_line(source, "missing"), None);
    }
    #[test]
    fn node_lines_ignore_ids_inside_labels() {
        assert_eq!(node_line("a[open the pay page]\npay --> b", "pay"), Some(2));
        for label in ["[pay]", "(pay)", "{pay}", "\"pay\"", "|pay|"] {
            assert_eq!(node_line(&format!("a{label}\npay --> b"), "pay"), Some(2));
        }
    }

    #[test]
    fn check_flow_reports_anchor_problems_without_the_filesystem() {
        let text = VALID.replace("```\n", "%% @code missing src/pay.rs\n%% @code pay\n```\n");
        let problems = check_flow("checkout.md", &text);
        assert_eq!(
            problems
                .iter()
                .map(|problem| problem.code.as_str())
                .collect::<Vec<_>>(),
            ["anchor-unknown-step", "anchor-syntax"]
        );
        assert_eq!(problems[0].line, Some(11));
        assert_eq!(problems[1].line, Some(12));
    }

    #[test]
    fn anchor_syntax_is_reported_outside_the_first_mermaid_block() {
        let text = format!(
            "%% @code pay src/pay.rs\n{VALID}%%@code pay src/pay.rs\n```mermaid\nflowchart TD\n%% @code pay src/pay.rs\n```\n```text\n%% @code pay src/pay.rs\n```\n"
        );
        let problems = check_flow("checkout.md", &text);
        let syntax: Vec<_> = problems
            .iter()
            .filter(|problem| problem.code.as_str() == "anchor-syntax")
            .collect();
        assert_eq!(
            syntax
                .iter()
                .map(|problem| problem.line)
                .collect::<Vec<_>>(),
            [Some(1), Some(13), Some(16), Some(19)]
        );
        assert!(
            syntax.iter().all(
                |problem| problem.message == "anchors belong in the flow's first mermaid block"
            )
        );
    }

    #[test]
    fn anchors_are_checked_even_when_the_diagram_does_not_parse() {
        for header in ["flowchart TD\na -->", "sequenceDiagram"] {
            let text = format!(
                "# Broken\nDescription.\n```mermaid\n{header}\n%% @code missing src/pay.rs\n%% @code pay\n```\n"
            );
            let problems = check_flow("broken.md", &text);
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.code.as_str() == "anchor-syntax")
            );
            assert!(
                !problems
                    .iter()
                    .any(|problem| problem.code.as_str() == "anchor-unknown-step")
            );
        }
    }

    #[test]
    fn label_excerpt_limits_characters_without_splitting_unicode() {
        assert_eq!(label_excerpt("Short"), "Short");
        assert_eq!(
            label_excerpt(&"界".repeat(LABEL_EXCERPT_CHARS)),
            "界".repeat(LABEL_EXCERPT_CHARS)
        );
        assert_eq!(
            label_excerpt(&"界".repeat(LABEL_EXCERPT_CHARS + 1)),
            format!("{}…", "界".repeat(LABEL_EXCERPT_CHARS))
        );
    }
}
