use std::fmt::Write as _;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use casemap::{check, coverage, flows, git};
use serde_json::{Value, json};

const STOP_COMMAND: &str = "casemap hook stop";
const HOOK_TIMEOUT_SECONDS: u64 = 30;
const MAX_HOOK_INPUT_BYTES: u64 = 1 << 20;
const MAX_LISTED_ITEMS: usize = 15;
const MAX_SESSION_CHARS: usize = 64;

struct StopInput {
    active: bool,
    cwd: Option<PathBuf>,
    session: Option<String>,
}

#[derive(Default)]
struct Warnings {
    identity: String,
    reason: String,
}

pub(super) fn stop(catalog: Option<&Path>) {
    let _ = try_stop(catalog);
}

fn try_stop(catalog: Option<&Path>) -> anyhow::Result<()> {
    let Some(input) = read_stop_input() else {
        return Ok(());
    };
    inject_test_panic();
    if input.active {
        return Ok(());
    }
    let cwd = input.cwd.map_or_else(std::env::current_dir, Ok)?;
    let git::Location::Inside { root, .. } = git::locate(&cwd)? else {
        return Ok(());
    };
    let directory = root.join(catalog.unwrap_or_else(|| Path::new(crate::DEFAULT_FLOWS_DIR)));
    if !directory.is_dir() {
        return Ok(());
    }
    let memory = git::git_path(&root, &memory_name(input.session.as_deref()))?;
    let warnings = collect_warnings(&root, &directory)?;
    remember_and_block(&memory, warnings);
    Ok(())
}

/// Test-only panic injection lets CLI tests verify the silent panic boundary.
fn inject_test_panic() {
    if std::env::var("CASEMAP_HOOK_TEST_PANIC").as_deref() == Ok("1") {
        panic!("injected hook panic");
    }
}

fn read_stop_input() -> Option<StopInput> {
    let stdin = io::stdin();
    read_stop_input_from(&stdin, || stdin.is_terminal())
}

fn read_stop_input_from(
    reader: impl Read,
    is_terminal: impl FnOnce() -> bool,
) -> Option<StopInput> {
    if is_terminal() {
        return None;
    }
    let mut text = String::new();
    reader
        .take(MAX_HOOK_INPUT_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let document: Value = serde_json::from_str(&text).ok()?;
    Some(StopInput {
        active: document
            .get("stop_hook_active")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cwd: document
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        session: document
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn memory_name(session: Option<&str>) -> String {
    let sanitized: String = session
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(MAX_SESSION_CHARS)
        .collect();
    let session = if sanitized.is_empty() {
        "default"
    } else {
        &sanitized
    };
    format!("casemap/stop-hook-{session}")
}

fn collect_warnings(root: &Path, catalog: &Path) -> anyhow::Result<Warnings> {
    let current = crate::load_catalog(catalog)?;
    let compared = crate::base_changes(catalog, "HEAD", &current)?;
    let changed = if crate::has_code_anchors(&current) {
        compared.changed
    } else {
        git::changed_files(root, &compared.base)?
    };
    if changed.is_empty() {
        return Ok(Warnings::default());
    }
    let report = crate::check_report(catalog)?;
    let uncovered = if flows::has_known_anchors(&current) {
        let changed = crate::changed_code(
            &changed,
            &compared.prefix,
            &coverage::Selection::default(),
            &|path| root.join(path).is_file(),
        );
        coverage::uncovered_changes(&changed, &current)
    } else {
        Vec::new()
    };
    let catalog_display = compared.prefix.trim_end_matches('/');
    Ok(Warnings {
        identity: warning_identity(&report, &compared.stale, &uncovered),
        reason: warning_reason(catalog_display, &report, &compared.stale, &uncovered),
    })
}

fn remember_and_block(memory_path: &Path, warnings: Warnings) {
    if warnings.identity.is_empty() {
        let _ = std::fs::remove_file(memory_path);
        return;
    }
    if std::fs::read_to_string(memory_path).is_ok_and(|previous| previous == warnings.identity) {
        return;
    }
    let _ = crate::create_parent(memory_path)
        .and_then(|()| std::fs::write(memory_path, &warnings.identity));
    let _ = writeln!(
        io::stdout().lock(),
        "{}",
        json!({"decision": "block", "reason": warnings.reason})
    );
}

fn warning_identity(
    report: &check::Report,
    stale: &[flows::Stale],
    uncovered: &[String],
) -> String {
    let mut identities = Vec::new();
    for problem in &report.problems {
        if problem.code.severity() == check::Severity::Error {
            identities.push(format!(
                "check:{}:{}:{}",
                problem.file.as_deref().unwrap_or_default(),
                problem.line.unwrap_or_default(),
                problem.code.as_str()
            ));
        }
    }
    for entry in stale {
        for hit in &entry.hits {
            let mut files = hit.files.clone();
            files.sort();
            let target = match &hit.target {
                casemap::anchors::Target::Flow => "*",
                casemap::anchors::Target::Step(step) => step,
            };
            identities.push(format!(
                "stale:{}:{target}:{}",
                entry.flow.path,
                files.join(":")
            ));
        }
    }
    for file in uncovered {
        identities.push(format!("uncovered:{file}"));
    }
    identities.sort();
    identities.join("\n")
}

fn write_remaining(reason: &mut String, total: usize) {
    if total > MAX_LISTED_ITEMS {
        let _ = writeln!(reason, "- … and {} more", total - MAX_LISTED_ITEMS);
    }
}

fn warning_reason(
    catalog: &str,
    report: &check::Report,
    stale: &[flows::Stale],
    uncovered: &[String],
) -> String {
    let mut sections = String::new();
    if report.errors() > 0 {
        sections.push_str("Check errors (run `casemap check`):\n");
        for problem in report
            .problems
            .iter()
            .filter(|problem| problem.code.severity() == check::Severity::Error)
            .take(MAX_LISTED_ITEMS)
        {
            let _ = writeln!(
                sections,
                "- {}: {} [{}]",
                crate::problem_location(Path::new(catalog), problem),
                problem.message,
                problem.code.as_str()
            );
        }
        write_remaining(&mut sections, report.errors());
    }
    if !stale.is_empty() {
        sections
            .push_str("Possibly stale flows (their anchored code changed, the flow did not):\n");
        for entry in stale.iter().take(MAX_LISTED_ITEMS) {
            let hits = entry
                .hits
                .iter()
                .map(flows::describe_stale_hit)
                .collect::<Vec<_>>();
            let _ = writeln!(
                sections,
                "- {} ({catalog}/{}): {}",
                entry.flow.title,
                entry.flow.path,
                hits.join("; ")
            );
        }
        write_remaining(&mut sections, stale.len());
        sections.push_str("  Update the flow, or say in your reply that it still holds.\n");
    }
    if !uncovered.is_empty() {
        sections.push_str("Changed code no flow covers:\n");
        for file in uncovered.iter().take(MAX_LISTED_ITEMS) {
            let _ = writeln!(sections, "- {file}");
        }
        write_remaining(&mut sections, uncovered.len());
        sections.push_str(
            "  Anchor it with `%% @code` in the flow it serves, or say why no flow applies.\n",
        );
    }
    if sections.is_empty() {
        return String::new();
    }
    format!("casemap: before you finish, keep the use-case map in {catalog}/ true.\n{sections}")
}

#[derive(Debug, PartialEq, Eq)]
enum Merge {
    Unchanged,
    Updated(String),
}

fn merge_stop_hook(existing: Option<&str>, command: &str, force: bool) -> Result<Merge, String> {
    let mut document: Value = match existing {
        Some(text) => serde_json::from_str(text).map_err(|error| error.to_string())?,
        None => json!({}),
    };
    let document_object = document.as_object_mut().ok_or("not a JSON object")?;
    let hooks = document_object
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("hooks is not a JSON object")?;
    let groups = hooks
        .entry("Stop")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("hooks.Stop is not an array")?;
    let mut present = false;
    let mut changed = false;
    for group in groups.iter_mut() {
        let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        for entry in entries {
            let Some(current) = entry.get_mut("command") else {
                continue;
            };
            if current.as_str().is_some_and(is_casemap_stop) {
                present = true;
                if force && current.as_str() != Some(command) {
                    *current = Value::String(command.to_string());
                    changed = true;
                }
            }
        }
    }
    if present && !changed {
        return Ok(Merge::Unchanged);
    }
    if !present {
        groups.push(casemap_entry(command));
    }
    serde_json::to_string_pretty(&document)
        .map(|text| Merge::Updated(format!("{text}\n")))
        .map_err(|error| error.to_string())
}

fn is_casemap_stop(command: &str) -> bool {
    let mut tokens = command.split_whitespace();
    let Some(program) = tokens.next() else {
        return false;
    };
    let filename = program.rsplit(['/', '\\']).next().unwrap_or_default();
    matches!(filename, "casemap" | "casemap.exe")
        && tokens.next() == Some("hook")
        && tokens.next() == Some("stop")
}

fn casemap_entry(command: &str) -> Value {
    json!({"hooks": [{"type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECONDS}]})
}

fn stop_command(catalog: &str) -> String {
    if catalog == crate::DEFAULT_FLOWS_DIR {
        return STOP_COMMAND.to_string();
    }
    let needs_quotes = catalog.chars().any(|character| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '.' | '/' | '-')
    });
    if needs_quotes {
        return format!(
            "{STOP_COMMAND} --flows '{}'",
            catalog.replace('\'', "'\\''")
        );
    }
    format!("{STOP_COMMAND} --flows {catalog}")
}

pub(super) fn install_hooks(root: &Path, catalog: &str, force: bool) -> bool {
    let command = stop_command(catalog);
    let mut failed = false;
    for (relative, label) in [
        (".claude/settings.json", "Claude Code hook"),
        (".codex/hooks.json", "Codex hook"),
    ] {
        if let Err(error) = install_hook(root, Path::new(relative), &command, force, label) {
            eprintln!("error: {relative}: {error}");
            failed = true;
        }
    }
    failed
}

fn install_hook(
    root: &Path,
    relative: &Path,
    command: &str,
    force: bool,
    label: &str,
) -> anyhow::Result<()> {
    crate::check_skill_symlinks(root, relative)?;
    crate::ensure_inside_root(root, relative).map_err(|_| {
        anyhow::anyhow!(
            "refusing to write {} through a symlink outside the repository",
            crate::slash_path(relative)
        )
    })?;
    let path = root.join(relative);
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let merged =
        merge_stop_hook(existing.as_deref(), command, force).map_err(anyhow::Error::msg)?;
    let display = crate::slash_path(relative);
    match merged {
        Merge::Unchanged => println!("up to date {display}"),
        Merge::Updated(text) => {
            crate::write_skill(&path, &text)?;
            if existing.is_some() {
                println!("updated {display}");
            } else {
                println!("installed {display} ({label})");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use casemap::{anchors::Target, check, flows};

    fn updated(existing: Option<&str>) -> serde_json::Value {
        let Merge::Updated(text) = merge_stop_hook(existing, "casemap hook stop", false).unwrap()
        else {
            panic!("expected update");
        };
        assert!(text.ends_with('\n'));
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn merge_creates_missing_objects_and_preserves_other_hooks() {
        for existing in [
            None,
            Some("{}"),
            Some(r#"{"hooks":{"Stop":[{"hooks":[{"command":"other"}]}]}}"#),
        ] {
            let value = updated(existing);
            let groups = value["hooks"]["Stop"].as_array().unwrap();
            assert_eq!(
                groups.last().unwrap()["hooks"][0],
                serde_json::json!({"type":"command","command":"casemap hook stop","timeout":30})
            );
            if groups.len() == 2 {
                assert_eq!(groups[0]["hooks"][0]["command"], "other");
            }
        }
    }

    #[test]
    fn merge_keeps_existing_command_unless_forced() {
        let existing = r#"{"hooks":{"Stop":[{"hooks":[{"command":"other"},{"command":"casemap hook stop --flows old","timeout":42}]}]}}"#;
        assert_eq!(
            merge_stop_hook(Some(existing), "casemap hook stop", false).unwrap(),
            Merge::Unchanged
        );
        let Merge::Updated(text) =
            merge_stop_hook(Some(existing), "casemap hook stop", true).unwrap()
        else {
            panic!("expected update")
        };
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value["hooks"]["Stop"][0]["hooks"][1]["command"],
            "casemap hook stop"
        );
        assert_eq!(value["hooks"]["Stop"][0]["hooks"][1]["timeout"], 42);
        assert_eq!(value["hooks"]["Stop"][0]["hooks"][0]["command"], "other");
        assert_eq!(
            merge_stop_hook(Some(&text), "casemap hook stop", true).unwrap(),
            Merge::Unchanged
        );
    }

    #[test]
    fn merge_rejects_invalid_shapes() {
        for (existing, expected) in [
            ("[]", "not a JSON object"),
            (r#"{"hooks":[]}"#, "hooks is not a JSON object"),
            (r#"{"hooks":{"Stop":{}}}"#, "hooks.Stop is not an array"),
        ] {
            assert_eq!(
                merge_stop_hook(Some(existing), "casemap hook stop", false).unwrap_err(),
                expected
            );
        }
        assert!(merge_stop_hook(Some("invalid"), "casemap hook stop", false).is_err());
    }

    #[test]
    fn stop_entry_detection_is_exact() {
        for command in [
            "casemap hook stop",
            "/usr/local/bin/casemap hook stop",
            "casemap hook stop --flows x",
            r"C:\bin\casemap.exe hook stop",
        ] {
            assert!(is_casemap_stop(command), "{command}");
        }
        for command in [
            "casemap hook stopwatch",
            "other hook stop",
            "casemap hook",
            "",
        ] {
            assert!(!is_casemap_stop(command), "{command}");
        }
    }

    #[test]
    fn stop_command_quotes_only_shell_metacharacters() {
        assert_eq!(
            stop_command("docs/journeys"),
            "casemap hook stop --flows docs/journeys"
        );
        assert_eq!(
            stop_command("docs/my flows"),
            "casemap hook stop --flows 'docs/my flows'"
        );
    }

    #[test]
    fn terminal_input_returns_without_reading() {
        struct NeverRead;
        impl std::io::Read for NeverRead {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("terminal input must not be read");
            }
        }
        assert!(read_stop_input_from(NeverRead, || true).is_none());
    }

    #[test]
    fn session_memory_names_stay_inside_the_git_directory() {
        assert_eq!(memory_name(None), "casemap/stop-hook-default");
        assert_eq!(memory_name(Some("../../A b_1-")), "casemap/stop-hook-Ab_1-");
        assert_eq!(memory_name(Some("../")), "casemap/stop-hook-default");
        assert_eq!(
            memory_name(Some(&"a".repeat(100))),
            format!("casemap/stop-hook-{}", "a".repeat(64))
        );
    }

    #[test]
    fn reason_bounds_check_errors_and_stale_flows_independently() {
        let mut problems = Vec::new();
        let mut stale = Vec::new();
        for index in 0..100 {
            problems.push(check::Problem {
                file: Some(format!("{index}.md")),
                line: Some(3),
                code: check::Code::MissingDiagram,
                message: "missing diagram".into(),
            });
            stale.push(flows::Stale {
                flow: flows::parse_flow(&format!("{index}.md"), "# Pay\n\nPay an order.\n"),
                hits: vec![flows::StaleHit {
                    target: Target::Flow,
                    files: vec!["src/pay.rs".into()],
                }],
            });
        }
        let report = check::Report {
            flows: 100,
            problems,
        };
        let reason = warning_reason("docs/flows", &report, &stale, &[]);
        assert_eq!(
            reason.lines().filter(|line| line.starts_with("- ")).count(),
            32
        );
        assert_eq!(reason.matches("- … and 85 more").count(), 2);
    }

    #[test]
    fn warning_identity_uses_all_warnings_and_ignores_presentation() {
        let mut report = check::Report {
            flows: 1,
            problems: vec![check::Problem {
                file: Some("pay.md".into()),
                line: Some(3),
                code: check::Code::MissingDiagram,
                message: "missing diagram".into(),
            }],
        };
        let mut stale = vec![flows::Stale {
            flow: flows::parse_flow("pay.md", "# Pay\n\nPay an order.\n"),
            hits: vec![flows::StaleHit {
                target: Target::Flow,
                files: vec!["src/z.rs".into(), "src/a.rs".into()],
            }],
        }];
        let uncovered = vec!["src/new.rs".into()];
        let expected = "check:pay.md:3:missing-diagram\nstale:pay.md:*:src/a.rs:src/z.rs\nuncovered:src/new.rs";
        assert_eq!(warning_identity(&report, &stale, &uncovered), expected);
        report.problems[0].message = "reworded".into();
        stale[0].flow.title = "New title".into();
        stale[0].hits[0].files.reverse();
        assert_eq!(warning_identity(&report, &stale, &uncovered), expected);
    }

    #[test]
    fn merge_detects_absolute_commands_and_preserves_similarly_named_commands() {
        let existing = r#"{"hooks":{"Stop":[{"hooks":[{"command":"casemap hook stopwatch"},{"command":"/usr/local/bin/casemap hook stop --flows old"}]}]}}"#;
        assert_eq!(
            merge_stop_hook(Some(existing), STOP_COMMAND, false).unwrap(),
            Merge::Unchanged
        );
        let Merge::Updated(text) = merge_stop_hook(Some(existing), STOP_COMMAND, true).unwrap()
        else {
            panic!("expected update");
        };
        let document: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            document["hooks"]["Stop"][0]["hooks"][0]["command"],
            "casemap hook stopwatch"
        );
        assert_eq!(
            document["hooks"]["Stop"][0]["hooks"][1]["command"],
            STOP_COMMAND
        );
        let existing = r#"{"hooks":{"Stop":[{"hooks":[{"command":"casemap hook stopwatch"}]}]}}"#;
        let Merge::Updated(text) = merge_stop_hook(Some(existing), STOP_COMMAND, false).unwrap()
        else {
            panic!("expected update");
        };
        let document: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(document["hooks"]["Stop"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn reason_has_only_nonempty_sections() {
        let report = check::Report {
            flows: 1,
            problems: vec![check::Problem {
                file: Some("pay.md".into()),
                line: Some(3),
                code: check::Code::MissingDiagram,
                message: "missing diagram".into(),
            }],
        };
        let stale = vec![flows::Stale {
            flow: flows::parse_flow("pay.md", "# Pay\n\nPay an order.\n"),
            hits: vec![flows::StaleHit {
                target: Target::Flow,
                files: vec!["src/pay.rs".into()],
            }],
        }];
        let expected = "casemap: before you finish, keep the use-case map in docs/flows/ true.\nCheck errors (run `casemap check`):\n- docs/flows/pay.md:3: missing diagram [missing-diagram]\nPossibly stale flows (their anchored code changed, the flow did not):\n- Pay (docs/flows/pay.md): whole flow: src/pay.rs\n  Update the flow, or say in your reply that it still holds.\nChanged code no flow covers:\n- src/new.rs\n  Anchor it with `%% @code` in the flow it serves, or say why no flow applies.\n";
        assert_eq!(
            warning_reason("docs/flows", &report, &stale, &["src/new.rs".into()]),
            expected
        );
        let empty = check::Report {
            flows: 1,
            problems: vec![],
        };
        assert_eq!(warning_reason("docs/flows", &empty, &[], &[]), "");
        assert!(!warning_reason("docs/flows", &report, &[], &[]).contains("Possibly stale"));
        assert!(!warning_reason("docs/flows", &empty, &stale, &[]).contains("Check errors"));
        assert!(
            !warning_reason("docs/flows", &empty, &[], &["src/new.rs".into()])
                .contains("Possibly stale")
        );
    }
}
