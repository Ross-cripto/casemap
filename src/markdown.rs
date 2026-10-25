use std::path::Path;

#[derive(Debug)]
pub struct MermaidBlock {
    /// Headings inside code fences do not apply to subsequent blocks.
    pub heading: Option<String>,
    pub source: String,
    /// Parser line N maps to document line `line + N`.
    pub line: usize,
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
    let mut blocks = Vec::new();
    let mut nearest_heading = None;
    let mut lines = md.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        if let Some(open) = fence(line) {
            let is_mermaid = open
                .info
                .split_whitespace()
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("mermaid"));
            let mut source = String::new();
            for (_, body) in lines.by_ref() {
                if closes(&open, body) {
                    break;
                }
                if is_mermaid {
                    source.push_str(body);
                    source.push('\n');
                }
            }
            if is_mermaid {
                blocks.push(MermaidBlock {
                    heading: nearest_heading.clone(),
                    source,
                    line: index + 1,
                });
            }
        } else if let Some(title) = heading(line) {
            nearest_heading = Some(title);
        }
    }
    blocks
}

pub fn first_heading(md: &str) -> Option<String> {
    let mut lines = md.lines();
    while let Some(line) = lines.next() {
        if let Some(open) = fence(line) {
            for body in lines.by_ref() {
                if closes(&open, body) {
                    break;
                }
            }
        } else if let Some(title) = heading(line) {
            return Some(title);
        }
    }
    None
}

pub fn first_paragraph_line(md: &str) -> Option<String> {
    let mut lines = md.lines();
    while let Some(line) = lines.next() {
        if let Some(open) = fence(line) {
            for body in lines.by_ref() {
                if closes(&open, body) {
                    break;
                }
            }
        } else if heading(line).is_some() {
            for body in lines {
                let text = body.trim();
                if text.is_empty() || text.starts_with("<!--") {
                    continue;
                }
                if heading(body).is_some() || fence(body).is_some() {
                    return None;
                }
                return Some(text.to_string());
            }
            return None;
        }
    }
    None
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
