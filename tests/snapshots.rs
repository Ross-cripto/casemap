mod common;

use casemap::flow_image::{ImageOptions, Theme, svg};
use casemap::{Options, flows};
use common::{CliRun, TempRepo, divergent_catalog, run_casemap, transcript};
use std::fs;
use std::path::Path;

fn cli(args: &[&str], env: &[(&str, &str)], input: Option<&str>) -> String {
    transcript(
        &args.join(" "),
        &run_casemap(args, Path::new(env!("CARGO_MANIFEST_DIR")), env, input),
    )
}

fn redact_shas(repo: &TempRepo, mut body: String) -> String {
    for query in [
        &["rev-parse", "HEAD"][..],
        &["rev-parse", "main"],
        &["merge-base", "HEAD", "main"],
    ] {
        let output = common::git_command(&repo.config_path())
            .args(query)
            .current_dir(repo.path())
            .output()
            .unwrap();
        if !output.status.success() {
            continue;
        }
        let sha = String::from_utf8(output.stdout).unwrap();
        body = body.replace(&sha[..flows::SHORT_SHA_LEN], "[SHA]");
    }
    body
}

fn repo_transcript(repo: &TempRepo, args: &[&str]) -> String {
    let run: CliRun = repo.casemap(args).into();
    redact_shas(repo, transcript(&args.join(" "), &run))
}

#[test]
fn svg_sectioned_catalog_flow() {
    let flow = flows::parse_flow(
        "check-the-catalog.md",
        include_str!("../docs/flows/check-the-catalog.md"),
    );
    let diagram = flows::diagram(&flow).unwrap().unwrap();
    let image = svg(&flow, &diagram, &Theme::dark(), ImageOptions::default());
    insta::assert_snapshot!("svg__sectioned_catalog_flow", image);
}

#[test]
fn svg_marked_checkout_comparison() {
    let before = flows::parse_flow(
        "checkout.md",
        include_str!("fixtures/flows/before/checkout.md"),
    );
    let after = flows::parse_flow(
        "checkout.md",
        include_str!("fixtures/flows/after/checkout.md"),
    );
    let previous = flows::diagram(&before).unwrap().unwrap();
    let current = flows::diagram(&after).unwrap().unwrap();
    let difference = casemap::diff::diff(&previous, &current);
    let options = ImageOptions {
        changes: Some(&difference.changes),
        ..ImageOptions::default()
    };
    let image = svg(&after, &difference.union, &Theme::dark(), options);
    insta::assert_snapshot!("svg__marked_checkout_comparison", image);
}

// pins: an explicit --color beats NO_COLOR
#[test]
fn cli_list() {
    insta::assert_snapshot!(
        "cli__list",
        cli(
            &[
                "flows",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn example_catalog_check() {
    insta::assert_snapshot!(
        "cli__example_check",
        cli(
            &[
                "check",
                "--strict",
                "--flows",
                "examples/commerce/docs/flows",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_unknown_flow() {
    insta::assert_snapshot!(
        "cli__unknown_flow",
        cli(
            &[
                "flows",
                "nosuch",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_missing_catalog() {
    insta::assert_snapshot!(
        "cli__missing_catalog",
        cli(&["flows", "--flows", "nope", "--no-color"], &[], None)
    );
}

#[test]
fn cli_guide() {
    insta::assert_snapshot!("cli__guide", cli(&["guide", "--no-color"], &[], None));
}

#[test]
fn cli_guide_custom() {
    insta::assert_snapshot!(
        "cli__guide_custom",
        cli(&["guide", "--flows", "flows", "--no-color"], &[], None)
    );
}

#[test]
fn cli_watch_no_path() {
    insta::assert_snapshot!("cli__watch_no_path", cli(&["-w"], &[], None));
}

#[test]
fn cli_watch_stdin() {
    insta::assert_snapshot!("cli__watch_stdin", cli(&["-w", "-"], &[], None));
}

#[test]
fn cli_watch_diff() {
    insta::assert_snapshot!("cli__watch_diff", cli(&["--watch", "diff"], &[], None));
}

#[test]
fn cli_invalid_diff_revision() {
    let repo = divergent_catalog();
    insta::assert_snapshot!(
        "cli__diff_invalid_revision",
        repo_transcript(&repo, &["diff", "-", "--no-color"])
    );
}

#[test]
fn cli_empty_catalog() {
    let repo = TempRepo::new();
    fs::create_dir(repo.file("empty")).unwrap();
    insta::assert_snapshot!(
        "cli__empty_catalog",
        repo_transcript(&repo, &["flows", "--flows", "empty", "--no-color"])
    );
}

#[test]
fn cli_flows_flag_before_subcommand() {
    insta::assert_snapshot!(
        "cli__flows_flag_before_subcommand",
        cli(
            &[
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color",
                "flows"
            ],
            &[],
            None
        )
    );
}

#[test]
fn cli_flows_repeated() {
    insta::assert_snapshot!(
        "cli__flows_repeated",
        cli(
            &[
                "--flows",
                "nope",
                "--flows",
                "tests/fixtures/flows/after",
                "--no-color",
                "flows"
            ],
            &[],
            None
        )
    );
}

fn catalog_with_check_problems() -> TempRepo {
    let repo = TempRepo::new();
    fs::write(repo.flow_path("Bad.md"), "# Checkout\n").unwrap();
    repo
}

#[test]
fn cli_check_text() {
    let repo = catalog_with_check_problems();
    insta::assert_snapshot!(
        "cli__check_text",
        repo_transcript(&repo, &["check", "--no-color"])
    );
}

#[test]
fn cli_check_json() {
    let repo = catalog_with_check_problems();
    insta::assert_snapshot!(
        "cli__check_json",
        repo_transcript(&repo, &["check", "--format", "json"])
    );
}

#[test]
fn cli_diff_stale() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "fn pay() { changed(); }\n").unwrap();
    insta::assert_snapshot!(
        "cli__diff_stale",
        repo_transcript(&repo, &["diff", "--no-color"])
    );
}

#[test]
fn cli_which() {
    let repo = common::anchored_catalog();
    insta::assert_snapshot!(
        "cli__which",
        repo_transcript(&repo, &["which", "src/pay.rs", "src/other.rs"])
    );
}

#[test]
fn diff_markdown() {
    let repo = divergent_catalog();
    insta::assert_snapshot!(
        "diff_markdown",
        repo_transcript(&repo, &["diff", "main", "--format", "markdown"])
    );
}

#[test]
fn diff_markdown_stale() {
    let repo = common::anchored_catalog();
    fs::write(repo.file("src/pay.rs"), "fn pay() { changed(); }\n").unwrap();
    insta::assert_snapshot!(
        "diff_markdown_stale",
        repo_transcript(&repo, &["diff", "--format", "markdown"])
    );
}

#[test]
fn diff_markdown_quiet() {
    let repo = common::anchored_catalog();
    insta::assert_snapshot!(
        "diff_markdown_quiet",
        repo_transcript(&repo, &["diff", "--format", "markdown"])
    );
}

#[test]
fn diff_markdown_broken() {
    let repo = common::anchored_catalog();
    fs::write(
        repo.flow_path("broken.md"),
        "# Broken\n\nDescription.\n\n```mermaid\ngraph TD\na -->\n```\n",
    )
    .unwrap();
    insta::assert_snapshot!(
        "diff_markdown_broken",
        repo_transcript(&repo, &["diff", "--format", "markdown"])
    );
}

#[test]
fn coverage_repository_text() {
    let repo = common::coverage_catalog();
    insta::assert_snapshot!(
        "coverage_repository_text",
        repo_transcript(&repo, &["coverage"])
    );
}

#[test]
fn coverage_branch_text() {
    let repo = common::coverage_catalog();
    insta::assert_snapshot!(
        "coverage_branch_text",
        repo_transcript(&repo, &["coverage", "main"])
    );
}

#[test]
fn diff_markdown_uncovered() {
    let repo = common::coverage_catalog();
    insta::assert_snapshot!(
        "diff_markdown_uncovered",
        repo_transcript(&repo, &["diff", "main", "--format", "markdown"])
    );
}

fn proposal_transcript(args: &[&str]) -> String {
    let repo = TempRepo::committed_before();
    let proposal = fs::read_to_string("tests/fixtures/flows/after/checkout.md").unwrap();
    let run: CliRun = repo.casemap_stdin(args, &proposal).into();
    redact_shas(&repo, transcript(&args.join(" "), &run))
}

#[test]
fn cli_propose_markdown() {
    insta::assert_snapshot!(
        "cli__propose_markdown",
        proposal_transcript(&["propose", "checkout", "--no-color", "--format", "markdown"])
    );
}

#[test]
fn map_layouts() {
    let directory = common::map_catalog();
    let catalog = directory.path().join("docs/flows");
    let flows = flows::load_dir(&catalog).unwrap();
    let map = casemap::map::load_map(&catalog).unwrap().unwrap();
    let groups = casemap::map::group_flows(&map, &flows);
    let grid = casemap::map::render_map(
        &groups,
        "docs/flows",
        casemap::map::MapLayout::Grid,
        80,
        &Options::default(),
    );
    assert_eq!(
        grid,
        concat!(
            " docs/flows · 9 use cases in 3 activities\n\n",
            " ╭─ Find products ──────╮   ╭─ Choose and buy ─────╮   ╭─ Sync the store ─────╮\n",
            " │ 4 use cases          │ ▶ │ 4 use cases          │ ▶ │ 1 use case           │\n",
            " ╰──────────────────────╯   ╰──────────────────────╯   ╰──────────────────────╯\n",
            " · Browse the storefront    · View a product and       · Refresh the catalogue\n",
            " · Browse a collection        choose options             after a store edit\n",
            " · Search for products      · Add an item to the cart\n",
            " · Read a store page        · Manage the cart\n",
            "                            · Check out a cart\n"
        )
    );
    insta::assert_snapshot!("map__grid", grid);
    insta::assert_snapshot!(
        "map__outline",
        casemap::map::render_map(
            &groups,
            "docs/flows",
            casemap::map::MapLayout::Outline,
            80,
            &Options::default()
        )
    );
}
