use std::collections::BTreeMap;

use crate::{
    anchors,
    flows::{self, Flow},
};

pub const CODE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "rb", "java", "kt", "kts", "swift",
    "cs", "php", "ex", "exs", "scala", "c", "h", "cc", "cpp", "hpp", "m", "mm", "dart", "lua",
    "vue", "svelte", "elm", "clj", "fs", "erl", "hs", "ml", "zig", "sh",
];
pub const EXCLUDED_SEGMENTS: &[&str] = &[
    "test",
    "tests",
    "__tests__",
    "spec",
    "specs",
    "vendor",
    "node_modules",
    "target",
    "dist",
    "build",
    ".github",
    ".git",
];

#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

pub fn is_code_path(path: &str, catalog_prefix: &str, selection: &Selection) -> bool {
    let catalog = catalog_prefix.trim_end_matches('/');
    let inside_catalog = path == catalog
        || path
            .strip_prefix(catalog)
            .is_some_and(|tail| tail.starts_with('/'));
    if !catalog.is_empty() && inside_catalog {
        return false;
    }
    if anchors::any_pattern_matches(&selection.exclude, path) {
        return false;
    }
    if !selection.include.is_empty() {
        return anchors::any_pattern_matches(&selection.include, path);
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    CODE_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(extension))
        && !path.split('/').any(|segment| {
            EXCLUDED_SEGMENTS
                .iter()
                .any(|excluded| excluded.eq_ignore_ascii_case(segment))
        })
        && !is_test_name(name, extension)
}

fn is_test_name(name: &str, extension: &str) -> bool {
    if name.contains("_test.") || name.contains(".test.") || name.contains(".spec.") {
        return true;
    }
    if extension == "py" && name.starts_with("test_") {
        return true;
    }
    extension == "rb" && name.ends_with("_spec.rb")
}

#[derive(Clone, Debug)]
pub struct Directory {
    pub path: String,
    pub files: usize,
    pub covered: usize,
    pub uncovered: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub files: usize,
    pub covered: usize,
    pub directories: Vec<Directory>,
    pub anchored: bool,
}

pub fn repository(
    files: &[String],
    flows: &[Flow],
    catalog_prefix: &str,
    selection: &Selection,
) -> Report {
    let patterns = flows::known_patterns(flows);
    let mut directories = BTreeMap::new();
    let mut report = Report {
        files: 0,
        covered: 0,
        directories: Vec::new(),
        anchored: flows::has_known_anchors(flows),
    };
    for path in files {
        if !is_code_path(path, catalog_prefix, selection) {
            continue;
        }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let directory = directories.entry(parent).or_insert_with(|| Directory {
            path: parent.to_string(),
            files: 0,
            covered: 0,
            uncovered: Vec::new(),
        });
        report.files += 1;
        directory.files += 1;
        if anchors::any_pattern_matches(&patterns, path) {
            report.covered += 1;
            directory.covered += 1;
        } else {
            directory.uncovered.push(name.to_string());
        }
    }
    for mut directory in directories.into_values() {
        directory.uncovered.sort();
        report.directories.push(directory);
    }
    report
}

pub fn uncovered_changes(changed: &[String], flows: &[Flow]) -> Vec<String> {
    let patterns = flows::known_patterns(flows);
    let mut uncovered: Vec<_> = changed
        .iter()
        .filter(|path| !anchors::any_pattern_matches(&patterns, path))
        .cloned()
        .collect();
    uncovered.sort();
    uncovered
}
