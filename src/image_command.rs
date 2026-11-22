use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use casemap::flow_image::{self, Color, ImageOptions, Theme};
use casemap::flows::{self, Flow, FlowChange};
use casemap::{Diagram, diff, git};

pub(crate) const IMAGE_SCALE: f32 = 2.0;
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const CONFIG_FILE: &str = "casemap.toml";
#[cfg(unix)]
const IMAGE_FILE_MODE: u32 = 0o644;

#[derive(Clone, Copy, Debug, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Preset {
    Dark,
    Light,
}

impl Preset {
    fn theme(self) -> Theme {
        match self {
            Self::Dark => Theme::dark(),
            Self::Light => Theme::light(),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Overrides<'a> {
    pub(crate) preset: Option<Preset>,
    pub(crate) background: Option<&'a Color>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Configuration {
    #[serde(alias = "theme")]
    render: RenderSettings,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RenderSettings {
    #[serde(alias = "base")]
    theme: Option<Preset>,
    background: Option<String>,
    card: Option<String>,
    text: Option<String>,
    muted: Option<String>,
    yes: Option<String>,
    no: Option<String>,
    added: Option<String>,
    changed: Option<String>,
    removed: Option<String>,
    sections: Option<Vec<String>>,
}

impl RenderSettings {
    fn apply(&self, theme: &mut Theme) -> anyhow::Result<()> {
        let colors = [
            ("background", &self.background, &mut theme.background),
            ("card", &self.card, &mut theme.card),
            ("text", &self.text, &mut theme.text),
            ("muted", &self.muted, &mut theme.muted),
            ("yes", &self.yes, &mut theme.yes),
            ("no", &self.no, &mut theme.no),
            ("added", &self.added, &mut theme.added),
            ("changed", &self.changed, &mut theme.changed),
            ("removed", &self.removed, &mut theme.removed),
        ];
        for (name, setting, color) in colors {
            if let Some(setting) = setting {
                *color = setting
                    .parse()
                    .with_context(|| format!("invalid render.{name}"))?;
            }
        }
        if let Some(sections) = &self.sections {
            if sections.is_empty() {
                bail!("render.sections requires at least one color");
            }
            theme.sections = sections
                .iter()
                .map(|color| color.parse())
                .collect::<Result<_, _>>()
                .context("invalid color in render.sections")?;
        }
        Ok(())
    }
}

pub(crate) fn theme(location: &Path, overrides: Overrides<'_>) -> anyhow::Result<Theme> {
    let directory = config_directory(location);
    let repo = directory.join(CONFIG_FILE);
    let user = user_config_path();
    let repo_settings = read_settings(&repo, Some(&directory))?;
    let user_settings = user
        .as_deref()
        .map(|path| read_settings(path, None))
        .transpose()?
        .unwrap_or_default();
    let preset = overrides
        .preset
        .or(user_settings.theme)
        .or(repo_settings.theme)
        .unwrap_or(Preset::Dark);
    let mut theme = preset.theme();
    repo_settings
        .apply(&mut theme)
        .with_context(|| repo.display().to_string())?;
    if let Some(user) = user {
        user_settings
            .apply(&mut theme)
            .with_context(|| user.display().to_string())?;
    }
    if let Some(background) = overrides.background {
        theme.background = background.clone();
    }
    Ok(theme)
}

fn config_directory(location: &Path) -> PathBuf {
    let directory = if location.is_dir() {
        location
    } else {
        parent_directory(location)
    };
    git::toplevel(directory).unwrap_or_else(|_| directory.to_owned())
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn user_config_path() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(directory).join("casemap/config.toml"));
    }
    std::env::home_dir().map(|directory| directory.join(".config/casemap/config.toml"))
}

fn read_config(path: &Path, root: Option<&Path>) -> anyhow::Result<Option<String>> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_ok() {
                bail!("{} is a broken symlink", path.display());
            }
            return Ok(None);
        }
        Err(error) => return Err(error).with_context(|| path.display().to_string()),
    };
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if let Some(root) = root {
        ensure_inside_repository(path, root)?;
    }
    read_capped(path).map(Some)
}

fn ensure_inside_repository(path: &Path, root: &Path) -> anyhow::Result<()> {
    // Containment is best-effort: path checks cannot close races with writers inside the repository.
    let canonical_path = std::fs::canonicalize(path).with_context(|| path.display().to_string())?;
    let canonical_root = std::fs::canonicalize(root).with_context(|| root.display().to_string())?;
    if !canonical_path.starts_with(canonical_root) {
        bail!("{} points outside the repository", path.display());
    }
    Ok(())
}

fn read_capped(path: &Path) -> anyhow::Result<String> {
    let file = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    if !file
        .metadata()
        .with_context(|| path.display().to_string())?
        .is_file()
    {
        bail!("{} is not a regular file", path.display());
    }
    let mut contents = Vec::new();
    // The extra byte detects an oversized file without reading the rest.
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut contents)
        .with_context(|| path.display().to_string())?;
    if contents.len() as u64 > MAX_CONFIG_BYTES {
        bail!("{} is larger than 64 KiB", path.display());
    }
    String::from_utf8(contents).with_context(|| path.display().to_string())
}

fn read_settings(path: &Path, root: Option<&Path>) -> anyhow::Result<RenderSettings> {
    let Some(contents) = read_config(path, root)? else {
        return Ok(RenderSettings::default());
    };
    let config: Configuration =
        toml::from_str(&contents).with_context(|| path.display().to_string())?;
    Ok(config.render)
}

pub(crate) fn read_flow(path: &Path) -> anyhow::Result<(Flow, Diagram)> {
    let source = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    parse_flow(path, &source).with_context(|| path.display().to_string())
}

pub(crate) fn parse_flow(path: &Path, source: &str) -> anyhow::Result<(Flow, Diagram)> {
    let flow = flows::parse_flow(&path.to_string_lossy(), source);
    let diagram = flow_image::parse_flow(&flow)?;
    Ok((flow, diagram))
}

#[derive(Clone, Copy)]
pub(crate) enum ExportFormat {
    Png,
    Svg,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }
}

pub(crate) fn export(
    path: &Path,
    format: ExportFormat,
    output: Option<&Path>,
    overrides: Overrides<'_>,
) -> anyhow::Result<ExitCode> {
    let (flow, diagram) = read_flow(path)?;
    let theme = theme(path, overrides)?;
    let svg = flow_image::svg(&flow, &diagram, &theme, ImageOptions::default());
    let data = match format {
        ExportFormat::Png => flow_image::png(&svg, IMAGE_SCALE)?,
        ExportFormat::Svg => flow_image::outlined_svg(&svg)?.into_bytes(),
    };
    let default_output = default_export_path(path, format)?;
    let output = output.unwrap_or(&default_output);
    if std::fs::canonicalize(output)
        .is_ok_and(|output| std::fs::canonicalize(path).is_ok_and(|input| output == input))
    {
        bail!("cannot overwrite the flow file with its exported image");
    }
    write_image(output, &data, ImageAccess::Export)
        .with_context(|| output.display().to_string())?;
    println!("{}", output.display());
    Ok(ExitCode::SUCCESS)
}

fn default_export_path(path: &Path, format: ExportFormat) -> anyhow::Result<PathBuf> {
    let mut file_name = path
        .file_stem()
        .context("flow file has no name")?
        .to_os_string();
    file_name.push(".");
    file_name.push(format.extension());
    Ok(Path::new(".").join(file_name))
}

pub(crate) enum ImageAccess {
    Export,
    Temporary,
}

pub(crate) fn write_image(output: &Path, data: &[u8], access: ImageAccess) -> io::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(parent_directory(output))?;
    file.write_all(data)?;
    if matches!(access, ImageAccess::Export) {
        set_export_permissions(file.as_file())?;
    }
    file.persist(output)?;
    Ok(())
}

fn set_export_permissions(file: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(IMAGE_FILE_MODE))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}

pub(crate) fn change_svg(
    change: &FlowChange,
    old_flow: Option<&Flow>,
    theme: &Theme,
    side_by_side: bool,
    base_label: &str,
) -> anyhow::Result<Option<String>> {
    match change {
        FlowChange::Modified { flow, diff } | FlowChange::New { flow, diff, .. } => {
            let new_diagram = validated(flow)?;
            let expects_base = matches!(
                change,
                FlowChange::Modified { .. }
                    | FlowChange::New {
                        base_broken: true,
                        ..
                    }
            );
            let previous = old_flow.filter(|_| expects_base);
            let old_diagram = previous.and_then(base_image);
            if expects_base && old_diagram.is_none() {
                return Ok(Some(flow_image::svg_with_missing_base(
                    flow,
                    &new_diagram,
                    theme,
                    side_by_side.then_some(base_label),
                )));
            }
            if side_by_side {
                return Ok(Some(flow_image::side_by_side_svg(
                    previous.zip(old_diagram.as_ref()),
                    Some((flow, &new_diagram)),
                    theme,
                    base_label,
                )));
            }
            Ok(Some(marked_image(flow, diff, theme)))
        }
        FlowChange::Removed { flow } => {
            let old_flow = old_flow.unwrap_or(flow);
            let Some(old_diagram) = base_image(old_flow) else {
                return Ok(None);
            };
            if side_by_side {
                return Ok(Some(flow_image::side_by_side_svg(
                    Some((old_flow, &old_diagram)),
                    None,
                    theme,
                    base_label,
                )));
            }
            let empty = Diagram {
                direction: old_diagram.direction,
                nodes: Vec::new(),
                edges: Vec::new(),
                clusters: Vec::new(),
                node_cluster: Vec::new(),
            };
            let difference = diff::diff(&old_diagram, &empty);
            Ok(Some(marked_image(old_flow, &difference, theme)))
        }
        FlowChange::Unchanged { .. } | FlowChange::Broken { .. } => Ok(None),
    }
}

fn validated(flow: &Flow) -> anyhow::Result<Diagram> {
    flow_image::parse_flow(flow).with_context(|| flow.path.clone())
}

fn base_image(flow: &Flow) -> Option<Diagram> {
    match flow_image::parse_flow(flow) {
        Ok(diagram) => Some(diagram),
        Err(error) => {
            eprintln!("casemap: {} has no base image ({error})", flow.path);
            None
        }
    }
}

fn marked_image(flow: &Flow, difference: &diff::DiagramDiff, theme: &Theme) -> String {
    let options = ImageOptions {
        changes: Some(&difference.changes),
        ..ImageOptions::default()
    };
    flow_image::svg(flow, &difference.union, theme, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_config_rejected(root: &Path, reason: &str) {
        let error = match theme(root, Overrides::default()) {
            Ok(_) => panic!("invalid config applied settings"),
            Err(error) => format!("{error:#}"),
        };
        assert!(
            error.contains(&root.join(CONFIG_FILE).display().to_string()),
            "{error}"
        );
        assert!(error.contains(reason), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn repo_config_outside_the_repository_is_rejected() {
        let root = assert_fs::TempDir::new().unwrap();
        let outside = assert_fs::TempDir::new().unwrap();
        let target = outside.path().join(CONFIG_FILE);
        std::fs::write(&target, "[render]\ntheme = 'light'\n").unwrap();
        std::os::unix::fs::symlink(target, root.path().join(CONFIG_FILE)).unwrap();
        assert_config_rejected(root.path(), "points outside the repository");
    }

    #[test]
    fn repo_config_must_be_a_regular_file() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::create_dir(root.path().join(CONFIG_FILE)).unwrap();
        assert_config_rejected(root.path(), "is not a regular file");
    }

    #[test]
    fn oversized_config_is_rejected() {
        let root = assert_fs::TempDir::new().unwrap();
        std::fs::write(
            root.path().join(CONFIG_FILE),
            format!("#{}", "x".repeat(64 * 1024)),
        )
        .unwrap();
        assert_config_rejected(root.path(), "is larger than 64 KiB");
    }

    #[cfg(unix)]
    #[test]
    fn dangling_config_symlink_is_reported() {
        let root = assert_fs::TempDir::new().unwrap();
        std::os::unix::fs::symlink("missing", root.path().join(CONFIG_FILE)).unwrap();
        assert_config_rejected(root.path(), "is a broken symlink");
    }

    #[test]
    fn user_config_rejects_directories_and_oversized_files() {
        let root = assert_fs::TempDir::new().unwrap();
        let path = root.path().join("config.toml");
        std::fs::create_dir(&path).unwrap();
        let error = read_config(&path, None).unwrap_err().to_string();
        assert!(error.contains(&path.display().to_string()), "{error}");
        assert!(error.contains("is not a regular file"), "{error}");
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, vec![b'x'; 64 * 1024 + 1]).unwrap();
        let error = read_config(&path, None).unwrap_err().to_string();
        assert!(error.contains(&path.display().to_string()), "{error}");
        assert!(error.contains("is larger than 64 KiB"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn default_export_preserves_non_utf8_stems() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let input = Path::new(OsStr::from_bytes(b"flows/checkout-\xff.md"));
        for (format, expected) in [
            (ExportFormat::Svg, b"./checkout-\xff.svg".as_slice()),
            (ExportFormat::Png, b"./checkout-\xff.png".as_slice()),
        ] {
            let output = default_export_path(input, format).unwrap();
            assert_eq!(output.as_os_str().as_bytes(), expected);
        }
    }

    #[test]
    fn an_unrenderable_base_is_reported_in_the_current_image() {
        let current = flows::parse_flow(
            "checkout.md",
            "# Checkout\n\n```mermaid\nflowchart TD\na[Updated checkout]\n```",
        );
        for source in [
            "flowchart TD\na -->",
            "flowchart TD\na((Previous checkout))",
        ] {
            let base = flows::parse_flow(
                "checkout.md",
                &format!("# Checkout\n\n```mermaid\n{source}\n```"),
            );
            let changes =
                flows::compare(std::slice::from_ref(&base), std::slice::from_ref(&current));
            for side_by_side in [false, true] {
                let image = change_svg(
                    &changes[0],
                    Some(&base),
                    &Theme::dark(),
                    side_by_side,
                    "HEAD",
                )
                .unwrap()
                .unwrap();
                assert!(image.contains(">base flow could not be drawn</text>"));
                assert!(image.contains(">Updated checkout</text>"));
                assert!(!image.contains(">New</text>"));
            }
        }
    }
}
