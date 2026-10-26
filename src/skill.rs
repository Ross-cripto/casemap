pub const SKILL_NAME: &str = "strata-flows";
pub const SKILL_MD: &str = include_str!("../skills/strata-flows/SKILL.md");
pub const DEFAULT_CATALOG: &str = "docs/flows";
pub const INSTALL_DIRS: [(&str, &str); 2] = [
    (".claude/skills", "Claude Code"),
    (".agents/skills", "Codex and other agents"),
];

/// Substitutes the catalog and, for custom catalogs, the root-run sentence with a --flows reminder.
pub fn render(catalog: &str) -> String {
    let rendered = SKILL_MD.replace(DEFAULT_CATALOG, catalog);
    if catalog == DEFAULT_CATALOG {
        return rendered;
    }
    rendered.replace(
        "Run strata from the repository root.",
        &format!(
            "Run strata from the repository root and pass `--flows {catalog}` to every command."
        ),
    )
}

pub fn body(catalog: &str) -> String {
    body_from_source(&render(catalog))
}

fn body_from_source(source: &str) -> String {
    let rendered = normalise(source);
    let without_front_matter = rendered
        .strip_prefix("---\n")
        .and_then(|text| text.split_once("\n---\n"))
        .map(|(_, body)| body)
        .unwrap_or(&rendered);
    without_front_matter.trim_start_matches('\n').to_string()
}

pub fn normalise(text: &str) -> String {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_quotes_the_step_limits() {
        assert!(body(DEFAULT_CATALOG).contains(&format!(
            "{} to {}",
            crate::check::MIN_STEPS,
            crate::check::MAX_STEPS
        )));
        assert!(body(DEFAULT_CATALOG).contains(&format!(
            "within {} characters",
            crate::check::MAX_LABEL_WIDTH
        )));
    }

    fn front_matter_fields() -> std::collections::BTreeMap<&'static str, &'static str> {
        let mut lines = SKILL_MD.lines();
        assert_eq!(lines.next(), Some("---"));
        let mut fields = std::collections::BTreeMap::new();
        for line in lines {
            if line == "---" {
                return fields;
            }
            let (key, value) = line.split_once(':').unwrap();
            assert!(
                [
                    "name",
                    "description",
                    "license",
                    "compatibility",
                    "metadata",
                    "allowed-tools"
                ]
                .contains(&key)
            );
            fields.insert(key, value.trim());
        }
        panic!("skill front matter has no closing ---");
    }

    #[test]
    fn skill_frontmatter_is_valid() {
        let fields = front_matter_fields();
        assert_eq!(fields["name"], SKILL_NAME);
        assert!(SKILL_NAME.len() <= 64);
        assert!(SKILL_NAME.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        }));
        assert!((1..=1024).contains(&fields["description"].chars().count()));
        assert!(fields["compatibility"].chars().count() <= 500);
        assert!(SKILL_MD.lines().count() < 500);
    }

    #[test]
    fn body_handles_crlf() {
        let rendered = format!("\u{feff}{}", render(DEFAULT_CATALOG).replace('\n', "\r\n"));
        let body = body_from_source(&rendered);
        assert!(body.starts_with("# strata flows\n"));
        assert!(!body.contains("\r\n"));
    }

    #[test]
    fn body_drops_front_matter_and_renders_catalog() {
        let text = body("web/flows");
        assert!(text.starts_with("# strata flows\n"));
        assert!(!text.contains("docs/flows"));
        assert!(text.contains("web/flows/"));
        assert!(render("web/flows").starts_with("---\n"));
        assert!(!render("web/flows").contains("docs/flows"));
        assert_eq!(normalise("\u{feff}one\r\ntwo\r\n"), "one\ntwo\n");
    }

    #[test]
    fn render_adds_flows_flag_for_custom_catalog() {
        assert!(render("web/flows").contains(
            "Run strata from the repository root and pass `--flows web/flows` to every command."
        ));
    }

    #[test]
    fn render_keeps_default_text() {
        assert_eq!(render(DEFAULT_CATALOG), SKILL_MD);
        assert!(render(DEFAULT_CATALOG).contains("Run strata from the repository root."));
    }
}
