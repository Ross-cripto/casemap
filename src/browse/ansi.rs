use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

pub(super) fn ansi_to_text(source: &str) -> Text<'static> {
    let mut style = Style::default();
    let mut lines = Vec::new();
    for line in source.lines() {
        let mut spans = Vec::new();
        let mut remaining = line;
        while !remaining.is_empty() {
            let Some(escape) = remaining.find('\x1b') else {
                spans.push(Span::styled(remaining.to_owned(), style));
                break;
            };
            if escape > 0 {
                spans.push(Span::styled(remaining[..escape].to_owned(), style));
            }
            remaining = &remaining[escape + 1..];
            let Some(sequence) = remaining.strip_prefix('[') else {
                continue;
            };
            let Some(end) = sequence.find(|character: char| ('@'..='~').contains(&character))
            else {
                break;
            };
            if sequence.as_bytes()[end] == b'm' {
                style = sgr(style, &sequence[..end]);
            }
            remaining = &sequence[end + 1..];
        }
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

fn sgr(mut style: Style, codes: &str) -> Style {
    for code in codes.split(';') {
        match code {
            "" | "0" => style = Style::default(),
            "1" => style = style.add_modifier(Modifier::BOLD),
            "3" => style = style.add_modifier(Modifier::ITALIC),
            "31" => style = style.fg(Color::Red),
            "32" => style = style.fg(Color::Green),
            "33" => style = style.fg(Color::Yellow),
            "90" => style = style.fg(Color::DarkGray),
            _ => {}
        }
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use casemap::{Direction, Options, diff, flows};
    use std::path::Path;

    fn plain(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn plain_text_has_lines_semantics() {
        assert_eq!(plain(&ansi_to_text("one\ntwo\n")), ["one", "two"]);
        assert!(ansi_to_text("").lines.is_empty());
        assert_eq!(plain(&ansi_to_text("a\n\n")), ["a", ""]);
        assert_eq!(plain(&ansi_to_text("a\r\nb")), ["a", "b"]);
    }

    #[test]
    fn each_sgr_code() {
        for (code, expected) in [
            ("0", Style::default()),
            ("1", Style::default().add_modifier(Modifier::BOLD)),
            ("3", Style::default().add_modifier(Modifier::ITALIC)),
            ("31", Style::default().fg(Color::Red)),
            ("32", Style::default().fg(Color::Green)),
            ("33", Style::default().fg(Color::Yellow)),
            ("90", Style::default().fg(Color::DarkGray)),
        ] {
            let text = ansi_to_text(&format!("\x1b[{code}mhello"));
            assert_eq!(plain(&text), ["hello"]);
            assert_eq!(text.lines[0].spans[0].style, expected);
        }
    }

    #[test]
    fn combined_codes_and_unknown_numbers() {
        let text = ansi_to_text("\x1b[1;3;32;999mhello");
        assert_eq!(
            text.lines[0].spans[0].style,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC)
        );
    }

    #[test]
    fn reset_mid_line() {
        let text = ansi_to_text("\x1b[31mred\x1b[0mplain");
        assert_eq!(plain(&text), ["redplain"]);
        assert_eq!(text.lines[0].spans[1].style, Style::default());
    }

    #[test]
    fn empty_reset() {
        let text = ansi_to_text("\x1b[1mbold\x1b[mplain");
        assert_eq!(text.lines[0].spans[1].style, Style::default());
    }

    #[test]
    fn non_sgr_csi_and_lone_escape() {
        assert_eq!(plain(&ansi_to_text("a\x1b[2Jb\x1bc\x1b")), ["abc"]);
    }

    fn assert_render_matches(render: impl Fn(&Options) -> String) {
        for direction in [
            None,
            Some(Direction::TopDown),
            Some(Direction::BottomUp),
            Some(Direction::LeftRight),
            Some(Direction::RightLeft),
        ] {
            for ascii in [false, true] {
                let options = Options {
                    direction,
                    ascii,
                    color: false,
                };
                let expected = render(&options);
                let styled = render(&Options {
                    color: true,
                    ..options
                });
                for sequence in styled.split("\x1b[").skip(1) {
                    let (codes, _) = sequence.split_once('m').expect("only SGR is rendered");
                    assert!(
                        codes.split(';').all(|code| matches!(
                            code,
                            "" | "0" | "1" | "3" | "31" | "32" | "33" | "90"
                        )),
                        "unknown style {codes}"
                    );
                }
                assert_eq!(
                    plain(&ansi_to_text(&styled)),
                    expected.lines().collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn converter_matches_plain_render() {
        let mut paths = Vec::new();
        for directory in ["examples", "tests/fixtures"] {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|extension| extension == "mmd") {
                    paths.push(path);
                }
            }
        }
        paths.sort();
        let count = paths.len();
        assert!(
            count >= 6,
            "converter sweep found only {count} Mermaid fixtures"
        );
        for path in paths {
            let source = std::fs::read_to_string(path).unwrap();
            assert_render_matches(|options| casemap::render_str(&source, options).unwrap());
        }
        let previous = flows::load_dir(Path::new("tests/fixtures/flows/before")).unwrap();
        let current = flows::load_dir(Path::new("tests/fixtures/flows/after")).unwrap();
        for (name, added) in [
            ("checkout", false),
            ("login", false),
            ("refund", true),
            ("legacy-export", true),
        ] {
            let difference = if added {
                let catalog = if name == "legacy-export" {
                    &previous
                } else {
                    &current
                };
                let flow = catalog.iter().find(|flow| flow.name == name).unwrap();
                diff::all_added(&flows::diagram(flow).unwrap().unwrap())
            } else {
                let old = previous.iter().find(|flow| flow.name == name).unwrap();
                let new = current.iter().find(|flow| flow.name == name).unwrap();
                diff::diff(
                    &flows::diagram(old).unwrap().unwrap(),
                    &flows::diagram(new).unwrap().unwrap(),
                )
            };
            assert_render_matches(|options| casemap::render_diagram(&difference.union, options));
        }
    }
}
