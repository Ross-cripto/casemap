#![cfg(feature = "cli")]

use assert_cmd::cargo::cargo_bin_cmd;
use casemap::flow_image::{ImageOptions, Theme, steps, svg};
use casemap::flows;
use predicates::str::contains;
use std::fs;

const CHECKOUT: &str = "# Checkout\n\nPay for an order.\n\n```mermaid\nflowchart TD\nsubgraph payment [Payment]\n    cart([Customer]) --> pay[Enter payment]\n    pay --> ok{Accepted?}\nend\nsubgraph confirmation [Confirmation]\n    receipt[Receipt]\nend\nok -->|yes| receipt\nok -->|no| pay\n```\n";

#[test]
fn exports_the_same_flow_as_svg_and_png() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    let svg = flow.with_extension("svg");
    let png = flow.with_extension("png");
    fs::write(&flow, CHECKOUT).unwrap();

    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .arg("export")
        .arg(&flow)
        .arg("--svg")
        .assert()
        .success();
    let svg_source = fs::read_to_string(svg).unwrap();
    assert!(svg_source.contains("<svg"));
    assert!(svg_source.contains("<path"));
    assert!(!svg_source.contains("<text"));

    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .arg("export")
        .arg(&flow)
        .arg("--png")
        .assert()
        .success();
    let png_bytes = fs::read(png).unwrap();
    assert!(png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(&png_bytes[12..16], b"IHDR");
}

#[test]
fn draws_branch_labels_and_return_links() {
    let flow = flows::parse_flow("checkout.md", CHECKOUT);
    let diagram = flows::diagram(&flow).unwrap().unwrap();
    let image = svg(&flow, &diagram, &Theme::dark(), ImageOptions::default());
    let document = resvg::usvg::roxmltree::Document::parse(&image).unwrap();
    let labels: Vec<String> = document
        .descendants()
        .filter(|node| node.has_tag_name("text"))
        .map(|node| {
            node.descendants()
                .filter(resvg::usvg::roxmltree::Node::is_text)
                .filter_map(|text| text.text())
                .collect()
        })
        .collect();
    for expected in ["yes", "no", "Back to Enter payment"] {
        assert!(labels.iter().any(|label| label == expected), "{expected}");
    }
}

#[test]
fn highlights_incoming_paths_and_dulls_unrelated_steps() {
    let flow = flows::parse_flow("checkout.md", CHECKOUT);
    let diagram = flows::diagram(&flow).unwrap().unwrap();
    let payment = diagram
        .nodes
        .iter()
        .position(|node| node.id == "pay")
        .unwrap();
    let decision = diagram
        .nodes
        .iter()
        .position(|node| node.id == "ok")
        .unwrap();
    let options = ImageOptions {
        selected_step: Some(payment),
        ..ImageOptions::default()
    };
    let image = svg(&flow, &diagram, &Theme::dark(), options);
    assert!(!card_is_dimmed(&image, "Customer"));
    assert!(!card_is_dimmed(&image, "Enter payment"));
    assert!(card_is_dimmed(&image, "Receipt"));
    let positions = steps(&flow, &diagram, options);
    let position = positions
        .iter()
        .find(|position| position.node_index == payment)
        .unwrap();
    assert_eq!(position.returns_from, [decision]);
}

fn card_is_dimmed(image: &str, label: &str) -> bool {
    let document = resvg::usvg::roxmltree::Document::parse(image).unwrap();
    let card_label = document
        .descendants()
        .find(|node| node.has_tag_name("text") && node.text() == Some(label))
        .expect("card label");
    card_label
        .ancestors()
        .any(|ancestor| ancestor.attribute("opacity") == Some(".40"))
}

#[test]
fn image_input_rejects_generic_shapes_links_and_multiple_diagrams() {
    let generic_shape = CHECKOUT.replace("pay[Enter payment]", "pay[(Enter payment)]");
    let generic_link = CHECKOUT.replace("pay --> ok", "pay -.-> ok");
    let multiple = format!("{CHECKOUT}# Other\n```mermaid\nflowchart TD\nz[Other]\n```\n");
    for text in [generic_shape, generic_link, multiple] {
        let flow = flows::parse_flow("checkout.md", &text);
        assert!(casemap::flow_image::parse_flow(&flow).is_err());
    }
}

#[test]
fn renders_to_a_temporary_png_when_stdout_is_redirected() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    let temp = directory.path().join("missing-temp-dir");
    fs::write(&flow, CHECKOUT).unwrap();

    let mut command = cargo_bin_cmd!("casemap");
    command.current_dir(directory.path());
    command
        .arg(&flow)
        .env("TMPDIR", &temp)
        .env("TMP", &temp)
        .env("TEMP", &temp);
    let result = command.assert().success();
    let notice = String::from_utf8(result.get_output().stdout.clone()).unwrap();
    let saved = notice
        .strip_prefix("Terminal graphics unavailable (not a terminal). Saved PNG to ")
        .and_then(|line| line.strip_suffix('\n'))
        .expect("one fallback notice");
    let path = std::path::PathBuf::from(saved);
    assert_eq!(path.parent().unwrap().parent(), Some(temp.as_path()));
    let first = fs::read(&path).unwrap();
    assert!(first.starts_with(b"\x89PNG\r\n\x1a\n"));

    fs::write(&flow, CHECKOUT.replace("Enter payment", "Retry payment")).unwrap();
    command.assert().success().stdout(notice);
    assert_ne!(fs::read(&path).unwrap(), first);
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn rendering_hashes_the_original_path_for_non_utf8_filenames() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let directory = assert_fs::TempDir::new().unwrap();
    let mut images = Vec::new();
    for byte in [0x80, 0x81] {
        let mut name = b"checkout-".to_vec();
        name.push(byte);
        name.extend_from_slice(b".md");
        let path = directory.path().join(OsString::from_vec(name));
        fs::write(&path, CHECKOUT).unwrap();
        let result = cargo_bin_cmd!("casemap")
            .current_dir(directory.path())
            .arg(path)
            .env("TMPDIR", directory.path())
            .assert()
            .success();
        let notice = String::from_utf8_lossy(&result.get_output().stdout);
        let (_, saved) = notice.trim_end().split_once("Saved PNG to ").unwrap();
        images.push(saved.to_owned());
    }
    assert_ne!(images[0], images[1]);
    assert!(
        images
            .iter()
            .all(|saved| std::path::Path::new(saved).is_file())
    );
}

#[test]
fn exporting_to_a_hard_link_preserves_the_flow_source() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    let output = directory.path().join("checkout.png");
    fs::write(&flow, CHECKOUT).unwrap();
    fs::hard_link(&flow, &output).unwrap();
    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .arg("export")
        .arg(&flow)
        .arg("-o")
        .arg(&output)
        .assert()
        .success();
    assert_eq!(fs::read_to_string(flow).unwrap(), CHECKOUT);
    assert!(fs::read(output).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn user_settings_and_flags_override_repository_background() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    let config = directory.path().join("user");
    let user_file = config.join("casemap/config.toml");
    fs::create_dir_all(user_file.parent().unwrap()).unwrap();
    fs::write(&flow, CHECKOUT).unwrap();
    fs::write(
        directory.path().join("casemap.toml"),
        "[render]\nbackground = '#123456'\n",
    )
    .unwrap();
    fs::write(&user_file, "[render]\nbackground = '#345678'\n").unwrap();
    for (flag, expected) in [
        (None, [0x34, 0x56, 0x78]),
        (Some("#456789"), [0x45, 0x67, 0x89]),
    ] {
        let mut command = cargo_bin_cmd!("casemap");
        command.current_dir(directory.path());
        command
            .args(["export", "--png"])
            .arg(&flow)
            .env("XDG_CONFIG_HOME", &config);
        if let Some(flag) = flag {
            command.args(["--bg", flag]);
        }
        command.assert().success();
        let data = fs::read(flow.with_extension("png")).unwrap();
        let pixmap = resvg::tiny_skia::Pixmap::decode_png(&data).unwrap();
        assert_eq!(&pixmap.data()[..3], &expected);
    }
}

#[test]
fn config_typo_names_the_file_and_key() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    let config = directory.path().join("casemap.toml");
    fs::write(&flow, CHECKOUT).unwrap();
    fs::write(&config, "[theme]\ncolour = '#fff'\n").unwrap();

    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .args(["export", "--svg"])
        .arg(&flow)
        .env(
            "XDG_CONFIG_HOME",
            directory.path().join("empty-user-config"),
        )
        .assert()
        .failure()
        .stderr(contains(config.to_string_lossy().into_owned()))
        .stderr(contains("colour"));
}

#[test]
fn repo_config_applies_without_user_config() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    fs::write(&flow, CHECKOUT).unwrap();
    fs::write(
        directory.path().join("casemap.toml"),
        "[theme]\nbase = 'light'\n",
    )
    .unwrap();

    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .args(["export", "--svg"])
        .arg(&flow)
        .env(
            "XDG_CONFIG_HOME",
            directory.path().join("empty-user-config"),
        )
        .assert()
        .success();
    let image = fs::read_to_string(flow.with_extension("svg")).unwrap();
    assert!(image.contains("fill=\"#f7f7fa\""));
}

#[test]
fn theme_and_bg_flags_combine() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    fs::write(&flow, CHECKOUT).unwrap();

    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .args(["export", "--svg", "--theme", "light", "--bg", "#123456"])
        .arg(&flow)
        .env(
            "XDG_CONFIG_HOME",
            directory.path().join("empty-user-config"),
        )
        .assert()
        .success();
    let image = fs::read_to_string(flow.with_extension("svg")).unwrap();
    for color in ["#123456", "#ffffff", "#20232b"] {
        assert!(image.contains(&format!("\"{color}\"")), "missing {color}");
    }
}

#[cfg(unix)]
#[test]
fn exports_readable_files() {
    use std::os::unix::fs::PermissionsExt;

    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    fs::write(&flow, CHECKOUT).unwrap();

    cargo_bin_cmd!("casemap")
        .current_dir(directory.path())
        .args(["export", "--svg"])
        .arg(&flow)
        .assert()
        .success();
    let mode = fs::metadata(flow.with_extension("svg"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
}

#[test]
fn checked_flows_can_be_drawn() {
    fn check_source(path: &str, source: &str) -> bool {
        let problems = casemap::check::check_flow(path, source);
        let has_error = problems
            .iter()
            .any(|problem| problem.code.severity() == casemap::check::Severity::Error);
        if !has_error {
            let flow = flows::parse_flow(path, source);
            assert!(
                casemap::flow_image::parse_flow(&flow).is_ok(),
                "{path}: {problems:?}"
            );
        }
        has_error
    }
    fn visit(directory: &std::path::Path, drawable: &mut usize, errors: &mut usize) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, drawable, errors);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                if check_source(&path.to_string_lossy(), &fs::read_to_string(&path).unwrap()) {
                    *errors += 1;
                } else {
                    *drawable += 1;
                }
            }
        }
    }
    let mut drawable_count = 0;
    let mut error_count = 0;
    for directory in [
        "docs/flows",
        "tests/fixtures/flows",
        "tests/fixtures/corpus",
    ] {
        visit(
            std::path::Path::new(directory),
            &mut drawable_count,
            &mut error_count,
        );
    }
    assert!(drawable_count > 0);
    assert!(error_count > 0);
    let mermaid = CHECKOUT
        .split("```mermaid\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    for (path, source, expected_error_codes) in [
        (
            "checkout.md",
            format!("{CHECKOUT}```mermaid\nflowchart TD\na[Other]\n```\n"),
            &["extra-diagram"][..],
        ),
        ("checkout.mmd", mermaid.to_owned(), &["mmd-flow"]),
        (
            "checkout.md",
            CHECKOUT.replace("pay[Enter payment]", "pay((Pay))"),
            &["unsupported-shape"],
        ),
        (
            "checkout.md",
            CHECKOUT.replace("cart([Customer]) -->", "cart([Customer]) -.->"),
            &["unsupported-link"],
        ),
    ] {
        assert!(check_source(path, &source));
        let error_codes: Vec<_> = casemap::check::check_flow(path, &source)
            .iter()
            .filter(|problem| problem.code.severity() == casemap::check::Severity::Error)
            .map(|problem| problem.code.as_str())
            .collect();
        assert_eq!(error_codes, expected_error_codes, "{path}");
        assert!(casemap::flow_image::parse_flow(&flows::parse_flow(path, &source)).is_err());
    }
    for (header, expected_codes) in [
        ("graph TD", &[][..]),
        ("flowchart TB", &[]),
        ("flowchart LR", &["not-top-down"]),
    ] {
        let source = CHECKOUT.replace("flowchart TD", header);
        assert!(!check_source("checkout.md", &source));
        let problems = casemap::check::check_flow("checkout.md", &source);
        let codes: Vec<_> = problems
            .iter()
            .map(|problem| problem.code.as_str())
            .collect();
        assert_eq!(codes, expected_codes);
    }
}

#[test]
fn exports_all_flowchart_directions_top_down() {
    let directory = assert_fs::TempDir::new().unwrap();
    let flow = directory.path().join("checkout.md");
    for header in [
        "graph TD",
        "flowchart TB",
        "flowchart LR",
        "flowchart RL",
        "flowchart BT",
        "graph",
    ] {
        fs::write(&flow, CHECKOUT.replace("flowchart TD", header)).unwrap();
        cargo_bin_cmd!("casemap")
            .current_dir(directory.path())
            .args(["export", "checkout.md"])
            .env(
                "XDG_CONFIG_HOME",
                directory.path().join("empty-user-config"),
            )
            .assert()
            .success();
        assert!(
            fs::read(directory.path().join("checkout.png"))
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
    }
}
