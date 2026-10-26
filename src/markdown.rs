use std::path::Path;

#[derive(Debug)]
pub struct MermaidBlock {
    /// Headings inside code fences do not apply to subsequent blocks.
    pub heading: Option<String>,
    pub source: String,
    /// Parser line N maps to document line `line + N`.
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct Outline {
    pub title: Option<(usize, String)>,
    pub description: Option<(usize, String)>,
    pub blocks: Vec<Block>,
}

#[derive(Debug)]
pub struct Block {
    /// The 1-based opening fence line, not the first line of its content.
    pub line: usize,
    pub info: String,
    pub source: String,
    /// False when the fence continues to the end of the document.
    pub closed: bool,
    pub heading: Option<String>,
}

pub fn outline(md: &str) -> Outline {
    let normalised = md
        .strip_prefix('\u{feff}')
        .unwrap_or(md)
        .replace("\r\n", "\n");
    let mut document = Outline::default();
    let mut nearest_heading = None;
    let has_front_matter = normalised.lines().next() == Some("---")
        && normalised.lines().nth(1).is_some_and(is_front_matter_key);
    let mut in_front_matter = false;
    let mut in_comment = false;
    let mut wants_description = false;
    let mut active: Option<(Fence<'_>, Block)> = None;
    for (index, raw) in normalised.lines().enumerate() {
        let line_number = index + 1;
        if index == 0 && has_front_matter {
            in_front_matter = true;
            continue;
        }
        if in_front_matter {
            if raw == "---" || raw == "..." {
                in_front_matter = false;
            }
            continue;
        }
        if let Some((open, block)) = &mut active {
            if closes(open, raw) {
                block.closed = true;
                if let Some((_, block)) = active.take() {
                    document.blocks.push(block);
                }
            } else {
                block.source.push_str(raw);
                block.source.push('\n');
            }
            continue;
        }
        let visible = without_comments(raw, &mut in_comment);
        if let Some(open) = fence(&visible) {
            wants_description = false;
            let block = Block {
                line: line_number,
                info: open.info.to_string(),
                source: String::new(),
                closed: false,
                heading: nearest_heading.clone(),
            };
            active = Some((
                Fence {
                    character: open.character,
                    length: open.length,
                    info: "",
                },
                block,
            ));
        } else if let Some(title) = heading(&visible) {
            nearest_heading = Some(title.clone());
            if document.title.is_none() {
                document.title = Some((line_number, title));
                wants_description = true;
            } else {
                wants_description = false;
            }
        } else if wants_description && !visible.trim().is_empty() {
            document.description = Some((line_number, visible.trim().to_string()));
            wants_description = false;
        }
    }
    if let Some((_, block)) = active {
        document.blocks.push(block);
    }
    document
}

fn is_front_matter_key(line: &str) -> bool {
    let Some((key, _)) = line.split_once(':') else {
        return false;
    };
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn without_comments(line: &str, in_comment: &mut bool) -> String {
    if !*in_comment && !line.trim_start().starts_with("<!--") {
        return line.to_string();
    }
    *in_comment = true;
    if let Some(end) = line.find("-->") {
        *in_comment = false;
        return line[end + 3..].to_string();
    }
    String::new()
}

pub(crate) fn is_mermaid(info: &str) -> bool {
    info.split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("mermaid"))
}

struct Fence<'a> {
    character: char,
    length: usize,
    info: &'a str,
}

fn fence(line: &str) -> Option<Fence<'_>> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let character = trimmed.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|&ch| ch == character).count();
    if length < 3 {
        return None;
    }
    Some(Fence {
        character,
        length,
        info: trimmed[length..].trim(),
    })
}

fn closes(open: &Fence<'_>, line: &str) -> bool {
    fence(line).is_some_and(|close| {
        close.character == open.character && close.length >= open.length && close.info.is_empty()
    })
}

pub fn is_markdown_path(path: &Path) -> bool {
    crate::paths::has_extension(path, &["md", "markdown"])
}

fn heading(line: &str) -> Option<String> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let level = trimmed.chars().take_while(|&ch| ch == '#').count();
    if !(1..=6).contains(&level) || !trimmed[level..].starts_with(' ') {
        return None;
    }
    let text = trimmed[level..].trim();
    let without_hashes = text.trim_end_matches('#');
    let text = if without_hashes.is_empty() || without_hashes.ends_with(' ') {
        without_hashes.trim_end()
    } else {
        text
    };
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

pub fn mermaid_blocks(md: &str) -> Vec<MermaidBlock> {
    outline(md)
        .blocks
        .into_iter()
        .filter(|block| is_mermaid(&block.info))
        .map(|block| MermaidBlock {
            heading: block.heading,
            source: block.source,
            line: block.line,
        })
        .collect()
}

pub fn first_heading(md: &str) -> Option<String> {
    outline(md).title.map(|(_, title)| title)
}

pub fn first_paragraph_line(md: &str) -> Option<String> {
    outline(md).description.map(|(_, description)| description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_edge_cases() {
        assert_eq!(heading("# "), None);
        assert_eq!(first_heading("# "), None);
        assert_eq!(
            crate::flows::parse_flow("fallback.md", "# ").title,
            "fallback"
        );
        assert_eq!(heading("# C#").as_deref(), Some("C#"));
        assert_eq!(heading("# Title ##").as_deref(), Some("Title"));
        assert_eq!(heading("# ###"), None);
    }

    #[test]
    fn extracts_mermaid_blocks_with_nearest_heading() {
        let blocks = mermaid_blocks(
            "# A ##\n\n```mermaid\ngraph TD\na --> b\n```\n## B\n  ~~~~MERMAID extra\ngraph LR\nc --> d\n  ~~~~~\n",
        );
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].heading.as_deref(), Some("A"));
        assert_eq!(blocks[0].line, 3);
        assert_eq!(blocks[1].heading.as_deref(), Some("B"));
        assert_eq!(blocks[1].line, 8);
        assert!(blocks[1].source.contains("c --> d"));
    }

    #[test]
    fn skips_other_code_blocks() {
        let text = "# Real\n````bash\n# x\n```mermaid\nnot a diagram\n````\n```mermaid\ngraph TD\na --> b\n```\n";
        let blocks = mermaid_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].heading.as_deref(), Some("Real"));
        assert_eq!(first_heading(text).as_deref(), Some("Real"));
        assert_eq!(first_paragraph_line(text), None);
        assert_eq!(
            first_heading("```bash\n# fake\n```\n# Actual").as_deref(),
            Some("Actual")
        );
    }

    #[test]
    fn unclosed_mermaid_fence_runs_to_end() {
        let blocks = mermaid_blocks("```mermaid\ngraph TD\na --> b");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].source, "graph TD\na --> b\n");
        assert_eq!(blocks[0].heading, None);
    }
}

#[cfg(test)]
mod outline_tests {
    use super::*;

    #[test]
    fn outline_skips_front_matter() {
        let document = outline("---\nname: test\n# Fake\n```mermaid\n---\n# Real\nDescription.\n");
        assert_eq!(document.title, Some((6, "Real".into())));
        assert_eq!(document.description, Some((7, "Description.".into())));
        assert!(document.blocks.is_empty());
        assert!(outline("---\nname: test\n# Hidden").title.is_none());
        assert_eq!(
            outline("---\nname: test\n...\n# Visible").title.unwrap().0,
            4
        );
    }

    #[test]
    fn outline_skips_html_comments() {
        let document = outline(
            " <!--\n# Hidden\n```mermaid\n-->\n# Real\n<!-- description comment\nmore hidden prose -->\nDescription.\n```mermaid\nflowchart TD\n```\n",
        );
        assert_eq!(document.title, Some((5, "Real".into())));
        assert_eq!(document.description, Some((8, "Description.".into())));
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].line, 9);
        assert_eq!(
            outline("<!-- hidden --># Visible").title.unwrap().1,
            "Visible"
        );
    }

    #[test]
    fn outline_skips_front_matter_comments_and_bom() {
        let text = format!(
            "\u{feff}{}",
            crlf("# Real\nDescription.\n```MERMAID extra\nflowchart TD\nactor([Actor])\n```\n")
        );
        let document = outline(&text);
        assert_eq!(document.title, Some((1, "Real".into())));
        assert_eq!(document.description, Some((2, "Description.".into())));
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].line, 3);
        assert_eq!(document.blocks[0].source, "flowchart TD\nactor([Actor])\n");
        assert!(document.blocks[0].closed);
    }

    #[test]
    fn outline_marks_unclosed_fences() {
        let document = outline("# Title\n```rust\n# Hidden\n```\n~~~mermaid\nflowchart TD\n");
        assert_eq!(document.blocks.len(), 2);
        assert!(document.blocks[0].closed);
        assert!(!document.blocks[1].closed);
        assert_eq!(document.blocks[1].heading.as_deref(), Some("Title"));
        assert_eq!(document.description, None);
    }

    #[test]
    fn outline_reports_lines() {
        let document = outline(
            "\n# Title\n\nDescription.\n## Details\n```text\n# Hidden\n```\n```mermaid\ngraph LR\n```\n",
        );
        assert_eq!(document.title, Some((2, "Title".into())));
        assert_eq!(document.description, Some((4, "Description.".into())));
        assert_eq!(document.blocks[0].line, 6);
        assert_eq!(document.blocks[1].line, 9);
        assert_eq!(document.blocks[1].heading.as_deref(), Some("Details"));
        assert_eq!(
            outline("# Title\n## Subtitle\nDescription").description,
            None
        );
    }

    fn crlf(text: &str) -> String {
        text.replace('\n', "\r\n")
    }

    #[test]
    fn inline_comment_marker_is_text() {
        for description in ["Use `<!--` here.", "Prose <!-- is plain text."] {
            let text = format!("# Title\n{description}\n```mermaid\nflowchart TD\n```\n");
            let document = outline(&text);
            assert_eq!(document.description, Some((2, description.into())));
            assert_eq!(document.blocks.len(), 1);
            assert!(document.blocks[0].closed);
        }
    }

    #[test]
    fn mermaid_info_must_be_the_word_mermaid() {
        for info in ["mermaid", "MERMAID extra", "mermaid\tconfig"] {
            assert!(is_mermaid(info), "{info}");
        }
        for info in ["mermaidjs", "mermaid-extra", "other mermaid", ""] {
            assert!(!is_mermaid(info), "{info}");
        }
    }

    #[test]
    fn leading_rule_is_not_front_matter() {
        for text in ["---\n# Title", "---\n\n# Title", "---\nnot a key:\n# Title"] {
            assert!(outline(text).title.is_some(), "{text}");
        }
    }
}
