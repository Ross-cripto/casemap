#[cfg_attr(not(feature = "cli"), allow(unused_imports))]
mod common;

use casemap::anchors::{
    self, Anchor, Target, flow_anchors, normalise_pattern, parse_anchors, pattern_matches,
};
use casemap::{Direction, Options, flows, git};
use std::fs;

#[test]
fn parses_step_and_flow_anchors() {
    let (anchors, errors) = parse_anchors(
        "flowchart TD\n%% @code pay src/pay.rs src/lib.rs\n%%@code * src/**\n%% @code pay tests/pay.rs",
        0,
    );
    assert!(errors.is_empty());
    assert_eq!(
        anchors,
        vec![
            Anchor {
                target: Target::Step("pay".into()),
                patterns: vec!["src/pay.rs".into(), "src/lib.rs".into()],
                line: 2
            },
            Anchor {
                target: Target::Flow,
                patterns: vec!["src/**".into()],
                line: 3
            },
            Anchor {
                target: Target::Step("pay".into()),
                patterns: vec!["tests/pay.rs".into()],
                line: 4
            },
        ]
    );
}

#[test]
fn anchor_lines_are_file_lines() {
    for (file, text, expected) in [
        (
            "pay.md",
            "# Pay\nDescription.\n```mermaid\nflowchart TD\n%% @code pay src/pay.rs\n```\n",
            5,
        ),
        ("pay.mmd", "flowchart TD\n%% @code pay src/pay.rs", 2),
    ] {
        assert_eq!(
            flow_anchors(&flows::parse_flow(file, text)).0[0].line,
            expected
        );
    }
}

#[test]
fn detects_only_the_code_token() {
    let (anchors, errors) = parse_anchors(
        "%%@code pay src/pay.rs\n%% @codebase x\n%% note\n%%{init}%%\n%% @code",
        0,
    );
    assert_eq!(anchors.len(), 1);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].line, 5);
    assert_eq!(
        errors[0].message,
        "@code needs a target and at least one path"
    );
}

#[test]
fn rejects_escaping_and_backslash_patterns() {
    for raw in [
        "/src",
        "../src",
        "src/../pay.rs",
        "src/./pay.rs",
        "C:/src",
        "C:",
        "./",
        "",
    ] {
        assert_eq!(
            normalise_pattern(raw).unwrap_err(),
            format!("\"{raw}\": paths are relative to the repository root and stay inside it")
        );
    }
    assert_eq!(
        normalise_pattern("src\\pay.rs").unwrap_err(),
        "\"src\\pay.rs\": use / in paths"
    );
    let (anchors, errors) = parse_anchors("%% @code pay ../bad src/pay.rs src\\bad", 0);
    assert_eq!(anchors[0].patterns, ["src/pay.rs"]);
    assert_eq!(errors.len(), 2);
}

#[test]
fn normalises_dot_slash_and_trailing_slash() {
    assert_eq!(
        normalise_pattern("./src//checkout///").unwrap(),
        "src/checkout"
    );
}

#[test]
fn star_stays_in_a_segment_and_globstar_crosses() {
    for (pattern, path, expected) in [
        ("src/*.rs", "src/pay.rs", true),
        ("src/*.rs", "src/nested/pay.rs", false),
        ("src/**/pay.rs", "src/pay.rs", true),
        ("src/**/pay.rs", "src/a/b/pay.rs", true),
        ("src/**.rs", "src/a/b/pay.rs", false),
        ("src/**.rs", "src/pay.rs", true),
        ("src/?ay.rs", "src/pay.rs", true),
        ("src/?ay.rs", "src/界ay.rs", true),
        ("src/?ay.rs", "src/abpay.rs", false),
        ("**", "src/a/b", true),
        ("src/**", "src", true),
        ("**/src/**/pay.?s", "a/src/b/pay.rs", true),
        ("src/*a?b", "src/xaayb", true),
        ("src/*a?b", "src/xac", false),
        ("src/*.rs", "lib/src/pay.rs", false),
    ] {
        assert_eq!(
            pattern_matches(pattern, path),
            expected,
            "{pattern}: {path}"
        );
    }
}

#[test]
fn brackets_and_braces_are_literal() {
    assert!(pattern_matches("app/[id]/page.tsx", "app/[id]/page.tsx"));
    assert!(!pattern_matches("app/[id]/page.tsx", "app/i/page.tsx"));
    assert!(pattern_matches("app/{id}/*.tsx", "app/{id}/page.tsx"));
    assert!(!pattern_matches("app/{id}/*.tsx", "app/id/page.tsx"));
}

#[test]
fn literal_directory_matches_everything_under_it() {
    assert!(pattern_matches("src/pay", "src/pay"));
    assert!(pattern_matches("src/pay", "src/pay/deleted.rs"));
    assert!(!pattern_matches("src/pay", "src/payment.rs"));
}

#[test]
fn walk_files_skips_dot_git() {
    let directory = assert_fs::TempDir::new().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    fs::write(directory.path().join(".git/config"), "").unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("src/pay.rs"), "").unwrap();
    fs::write(directory.path().join("root.rs"), "").unwrap();
    assert_eq!(
        anchors::walk_files(directory.path()).unwrap(),
        ["root.rs", "src/pay.rs"]
    );
}

#[cfg(unix)]
#[test]
fn walk_files_does_not_follow_directory_symlinks() {
    let directory = assert_fs::TempDir::new().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(directory.path().join("src/pay.rs"), "").unwrap();
    std::os::unix::fs::symlink(directory.path(), directory.path().join("src/loop")).unwrap();
    assert_eq!(
        anchors::walk_files(directory.path()).unwrap(),
        ["src/loop", "src/pay.rs"]
    );
}

#[test]
fn repo_files_lists_tracked_and_untracked_but_not_ignored() {
    let repo = common::TempRepo::committed_before();
    fs::write(repo.file(".gitignore"), "ignored\n").unwrap();
    fs::write(repo.file("ignored"), "").unwrap();
    fs::write(repo.file("new file.rs"), "").unwrap();
    fs::remove_file(repo.flow_path("login.md")).unwrap();
    let files = git::repo_files(repo.path()).unwrap();
    assert!(files.contains(&"docs/flows/checkout.md".into()));
    assert!(files.contains(&"new file.rs".into()));
    assert!(!files.contains(&"ignored".into()));
    assert!(!files.contains(&"docs/flows/login.md".into()));
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);
}

const WHOLE_FLOW_ANCHOR: &str = "%% @code * src/**\n";

fn insert_anchor_in_every_block(source: &str) -> String {
    let mut updated = source.to_owned();
    let blocks = casemap::markdown::mermaid_blocks(source);
    for block in blocks.into_iter().rev() {
        let content_start: usize = source
            .split_inclusive('\n')
            .take(block.line)
            .map(str::len)
            .sum();
        updated.insert_str(content_start + block.source.len(), WHOLE_FLOW_ANCHOR);
    }
    updated
}

#[test]
fn anchors_do_not_change_rendering() {
    let mut sources = Vec::new();
    for directory in [
        "examples",
        "tests/fixtures",
        "tests/fixtures/flows/before",
        "tests/fixtures/flows/after",
    ] {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "mmd" || extension == "md")
            {
                sources.push(path);
            }
        }
    }
    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        let is_markdown = path.extension().unwrap() == "md";
        let anchored = if is_markdown {
            insert_anchor_in_every_block(&source)
        } else {
            format!("{source}\n{WHOLE_FLOW_ANCHOR}")
        };
        assert_ne!(anchored, source);
        if is_markdown {
            assert!(
                anchored.contains(&format!("{WHOLE_FLOW_ANCHOR}```")),
                "{}",
                path.display()
            );
        }
        for direction in [
            Direction::TopDown,
            Direction::BottomUp,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            for (ascii, color) in [(false, false), (true, false), (false, true)] {
                let options = Options {
                    direction: Some(direction),
                    ascii,
                    color,
                };
                if is_markdown {
                    assert_eq!(
                        casemap::render_markdown(&source, &options).unwrap(),
                        casemap::render_markdown(&anchored, &options).unwrap(),
                        "{}",
                        path.display()
                    );
                } else {
                    assert_eq!(
                        casemap::render_str(&source, &options).unwrap(),
                        casemap::render_str(&anchored, &options).unwrap(),
                        "{}",
                        path.display()
                    );
                }
            }
        }
    }
    for (name, previous_side, current_side) in [
        ("checkout.md", "before", "after"),
        ("login.md", "before", "after"),
    ] {
        let previous =
            fs::read_to_string(format!("tests/fixtures/flows/{previous_side}/{name}")).unwrap();
        let current =
            fs::read_to_string(format!("tests/fixtures/flows/{current_side}/{name}")).unwrap();
        let union = |before: &str, after: &str| {
            let before = flows::diagram(&flows::parse_flow(name, before))
                .unwrap()
                .unwrap();
            let after = flows::diagram(&flows::parse_flow(name, after))
                .unwrap()
                .unwrap();
            casemap::diff::diff(&before, &after).union
        };
        let anchored_previous = common::append_step(&previous, "%% @code * src/**\n");
        let anchored_current = common::append_step(&current, "%% @code * src/**\n");
        for direction in [
            Direction::TopDown,
            Direction::BottomUp,
            Direction::LeftRight,
            Direction::RightLeft,
        ] {
            let options = Options {
                direction: Some(direction),
                ..Options::default()
            };
            assert_eq!(
                casemap::render_diagram(&union(&previous, &current), &options),
                casemap::render_diagram(&union(&anchored_previous, &anchored_current), &options)
            );
        }
    }
}

#[test]
fn changed_files_includes_deleted_untracked_and_both_sides_of_renames() {
    let repo = common::TempRepo::committed_before();
    repo.git(&["mv", "docs/flows/login.md", "docs/flows/sign-in.md"]);
    fs::write(repo.file("untracked.rs"), "").unwrap();
    fs::write(repo.file(".gitignore"), "ignored\n").unwrap();
    fs::write(repo.file("ignored"), "").unwrap();
    assert_eq!(
        git::changed_files(repo.path(), "HEAD").unwrap(),
        [
            ".gitignore",
            "docs/flows/login.md",
            "docs/flows/sign-in.md",
            "untracked.rs"
        ]
    );
}

#[test]
fn stale_flows_merge_targets_sort_files_and_skip_touched_flows_and_bad_anchors() {
    let source = "flowchart TD\npay[Pay] --> done[Done]\n%% @code done src/z.rs\n%% @code * src\n%% @code pay src/*.rs\n%% @code done src/a.rs\n%% @code invalid ../escape src/a.rs\n";
    let current = [
        flows::parse_flow("z.mmd", source),
        flows::parse_flow("a.mmd", source),
        flows::parse_flow("touched.mmd", source),
    ];
    let changed =
        ["src/z.rs", "src/a.rs", "docs/flows/touched.mmd", "src/a.rs"].map(str::to_string);
    let stale = flows::stale_flows(&current, "docs/flows/", &changed);
    assert_eq!(
        stale
            .iter()
            .map(|entry| entry.flow.path.as_str())
            .collect::<Vec<_>>(),
        ["a.mmd", "z.mmd"]
    );
    let hits = &stale[0].hits;
    assert_eq!(
        hits.iter()
            .map(|hit| hit.target.clone())
            .collect::<Vec<_>>(),
        [
            Target::Step("done".into()),
            Target::Flow,
            Target::Step("pay".into())
        ]
    );
    for hit in hits {
        assert_eq!(hit.files, ["src/a.rs", "src/z.rs"]);
    }
}

#[test]
fn stale_report_limits_files_and_colours_only_the_header() {
    let flow = flows::parse_flow("pay.mmd", "flowchart TD\npay[Pay]\n%% @code * src\n");
    let changed: Vec<_> = (0..7).map(|index| format!("src/{index}.rs")).collect();
    let stale = flows::stale_flows(&[flow], "", &changed);
    let options = Options {
        color: true,
        ..Options::default()
    };
    let report = flows::render_report(&[], &stale, "HEAD", "123456789", &options);
    assert_eq!(
        report,
        "no flow changes since HEAD (1234567)\n\n\x1b[33mpossibly stale: 1 flow whose anchored code changed but the flow did not\x1b[0m\n  pay (pay.mmd)\n    whole flow: src/0.rs, src/1.rs, src/2.rs, src/3.rs, src/4.rs, … (+2 more)\n"
    );
}

#[test]
fn whole_flow_anchors_remain_queryable_when_the_diagram_is_broken() {
    let flow = flows::parse_flow(
        "broken.mmd",
        "flowchart TD\na -->\n%% @code * src/pay.rs\n%% @code a src/pay.rs\n",
    );
    let catalog = [flow];
    let matches = flows::anchored_flows(&catalog, &["src/pay.rs".into()]);
    assert_eq!(matches.len(), 1);
    assert!(matches[0].whole_flow);
    assert!(matches[0].steps.is_empty());
}

#[test]
fn matcher_handles_pathological_globstars() {
    let path = vec!["a"; 40].join("/");
    let started = std::time::Instant::now();
    assert!(!pattern_matches("**/a/**/a/**/a/**/a/**/a/**/b", &path));
    assert!(started.elapsed() < std::time::Duration::from_millis(200));
}

#[test]
fn drive_prefix_only_in_first_segment() {
    assert_eq!(normalise_pattern("docs/a:b.md").unwrap(), "docs/a:b.md");
    assert_eq!(normalise_pattern("docs/C:/x").unwrap(), "docs/C:/x");
    assert!(normalise_pattern("C:").is_err());
    assert!(normalise_pattern("C:foo").is_err());
    assert!(normalise_pattern("c:/x").is_err());
    assert_eq!(normalise_pattern("**/**/a/**/**").unwrap(), "**/a/**");
}

#[cfg(unix)]
#[test]
fn walk_files_skips_unreadable_directories() {
    use std::os::unix::fs::PermissionsExt;
    let directory = assert_fs::TempDir::new().unwrap();
    let unreadable = directory.path().join("private");
    fs::create_dir(&unreadable).unwrap();
    fs::write(unreadable.join("hidden.rs"), "").unwrap();
    fs::write(directory.path().join("visible.rs"), "").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o0)).unwrap();
    let denied = fs::read_dir(&unreadable).is_err();
    let result = anchors::walk_files(directory.path());
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o700)).unwrap();
    if denied {
        assert_eq!(result.unwrap(), ["visible.rs"]);
    }
}

#[test]
fn stale_report_cap_boundaries() {
    for (count, suffix) in [
        (flows::MAX_STALE_FILES, ""),
        (flows::MAX_STALE_FILES + 1, ", … (+1 more)"),
    ] {
        let flow = flows::parse_flow("pay.mmd", "flowchart TD\npay[Pay]\n%% @code * src\n");
        let changed: Vec<_> = (0..count).map(|index| format!("src/{index}.rs")).collect();
        let stale = flows::stale_flows(&[flow], "", &changed);
        let report = flows::render_report(&[], &stale, "HEAD", "123456789", &Options::default());
        assert!(
            report.ends_with(&format!(
                "    whole flow: src/0.rs, src/1.rs, src/2.rs, src/3.rs, src/4.rs{suffix}\n"
            )),
            "{report}"
        );
    }
}
