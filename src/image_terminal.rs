use std::borrow::Cow;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use resvg::tiny_skia;

use crate::image_command::{IMAGE_SCALE, ImageAccess, write_image};

const DEFAULT_COLUMNS: u16 = 80;
const DEFAULT_CELL_WIDTH: u16 = 10;
const DEFAULT_CELL_HEIGHT: u16 = 20;
const KITTY_CHUNK: usize = 4_096;
const ITERM_CHUNK: usize = 16_384;
const ITERM_SINGLE_FILE_LIMIT: usize = 4 * 1_024 * 1_024;
const SIXEL_PIXEL_LIMIT: u64 = 32 * 1_024 * 1_024;
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const HASH_PREFIX_SHIFT: u32 = 32;
const MAX_PNG_FILENAME_BYTES: usize = 255;
#[cfg(unix)]
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
pub(crate) type GraphicsReason = Cow<'static, str>;

static NEXT_IMAGE_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Protocol {
    Kitty,
    Iterm,
    Sixel,
}

pub(crate) struct Terminal {
    protocol: Protocol,
    passthrough: bool,
    image_id: u32,
}

#[derive(Debug, thiserror::Error)]
#[error("{ITERM_LIMIT_REASON}")]
pub(crate) struct ItermSizeLimit;

pub(crate) const ITERM_LIMIT_REASON: &str = "image exceeds the iTerm2 inline file size limit";

enum ImageWritten {
    Shown,
    ItermSizeLimit,
}

pub(crate) fn display(svg: &str, flow_path: &Path, side_by_side: bool) -> anyhow::Result<()> {
    let mut output = io::stdout().lock();
    let fallback_reason = match Terminal::detect() {
        Err(reason) => reason,
        Ok(terminal) => match write_terminal_image(&terminal, svg, &mut output)? {
            ImageWritten::Shown => return Ok(()),
            ImageWritten::ItermSizeLimit => Cow::Borrowed(ITERM_LIMIT_REASON),
        },
    };
    write_fallback(
        &save_png(svg, flow_path, side_by_side)?,
        &fallback_reason,
        &mut output,
    )?;
    output.flush()?;
    Ok(())
}

fn write_terminal_image(
    terminal: &Terminal,
    svg: &str,
    output: &mut impl Write,
) -> anyhow::Result<ImageWritten> {
    let width = crossterm::terminal::size().map_or(DEFAULT_COLUMNS, |(width, _)| width);
    match terminal.write_image(svg, output, width, false) {
        Ok(()) => {
            writeln!(output)?;
            output.flush()?;
            Ok(ImageWritten::Shown)
        }
        Err(error) if error.is::<ItermSizeLimit>() => Ok(ImageWritten::ItermSizeLimit),
        Err(error) => Err(error),
    }
}

pub(crate) fn write_fallback(path: &Path, reason: &str, output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "Terminal graphics unavailable ({reason}). Saved PNG to {}",
        path.display()
    )
}

fn flow_stem(path: &Path) -> Cow<'_, str> {
    path.file_stem()
        .map_or(Cow::Borrowed("flow"), |stem| stem.to_string_lossy())
}

pub(crate) fn save_png(svg: &str, flow_path: &Path, side_by_side: bool) -> anyhow::Result<PathBuf> {
    save_png_in(svg, flow_path, side_by_side, &std::env::temp_dir())
}

fn save_png_in(
    svg: &str,
    flow_path: &Path,
    side_by_side: bool,
    temp_directory: &Path,
) -> anyhow::Result<PathBuf> {
    let png = casemap::flow_image::png(svg, IMAGE_SCALE)?;
    let canonical = canonical_flow_path(&std::path::absolute(flow_path)?)
        .with_context(|| format!("could not resolve {}", flow_path.display()))?;
    let directory = fallback_directory(&std::path::absolute(temp_directory)?)?;
    let stem = flow_stem(&canonical);
    let variant = if side_by_side { "-before-after" } else { "" };
    let prefix = path_hash_prefix(&canonical);
    let suffix = format!("{variant}-{prefix:08x}.png");
    let mut stem_end = stem.len().min(MAX_PNG_FILENAME_BYTES - suffix.len());
    while !stem.is_char_boundary(stem_end) {
        stem_end -= 1;
    }
    let path = directory.join(format!("{}{suffix}", &stem[..stem_end]));
    write_image(&path, &png, ImageAccess::Temporary)
        .with_context(|| format!("could not write temporary flow image to {}", path.display()))?;
    Ok(path)
}

fn path_hash_prefix(path: &Path) -> u64 {
    #[cfg(windows)]
    let bytes = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().flat_map(u16::to_le_bytes)
    };
    #[cfg(not(windows))]
    let bytes = path.as_os_str().as_encoded_bytes().iter().copied();

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
    }
    hash >> HASH_PREFIX_SHIFT
}

fn canonical_flow_path(path: &Path) -> io::Result<PathBuf> {
    match path.canonicalize() {
        Ok(canonical) => Ok(canonical),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Err(error);
            };
            Ok(canonical_flow_path(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn fallback_directory(temp_directory: &Path) -> anyhow::Result<PathBuf> {
    use std::fs::{self, DirBuilder, Permissions};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    // SAFETY: geteuid takes no arguments and just reads the process identity.
    let uid = unsafe { libc::geteuid() };
    let directory = temp_directory.join(format!("casemap-{uid}"));
    fs::create_dir_all(temp_directory)
        .with_context(|| format!("could not create {}", temp_directory.display()))?;
    match DirBuilder::new()
        .mode(PRIVATE_DIRECTORY_MODE)
        .create(&directory)
    {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).with_context(|| format!("could not create {}", directory.display()));
        }
    }
    let metadata = fs::symlink_metadata(&directory)
        .with_context(|| format!("could not inspect {}", directory.display()))?;
    anyhow::ensure!(
        metadata.is_dir(),
        "{} is not a directory",
        directory.display()
    );
    anyhow::ensure!(
        metadata.uid() == uid,
        "{} is not owned by the current user",
        directory.display()
    );
    if metadata.mode() & 0o777 != PRIVATE_DIRECTORY_MODE {
        fs::set_permissions(&directory, Permissions::from_mode(PRIVATE_DIRECTORY_MODE))
            .with_context(|| format!("could not make {} private", directory.display()))?;
    }
    Ok(directory)
}

#[cfg(not(unix))]
fn fallback_directory(temp_directory: &Path) -> anyhow::Result<PathBuf> {
    let directory = temp_directory.join("casemap");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("could not create {}", directory.display()))?;
    Ok(directory)
}

impl Terminal {
    #[cfg(test)]
    pub(crate) fn kitty_for_test() -> Self {
        Self {
            protocol: Protocol::Kitty,
            passthrough: false,
            image_id: 42,
        }
    }

    pub(crate) fn detect() -> Result<Self, GraphicsReason> {
        let (protocol, passthrough) = graphics_settings(
            io::stdout().is_terminal(),
            |name| std::env::var_os(name).map(|value| value.to_string_lossy().into_owned()),
            tmux_passthrough,
        )?;
        let index = NEXT_IMAGE_ID.fetch_add(1, Ordering::Relaxed);
        let image_id = std::process::id()
            .wrapping_mul(65_537)
            .wrapping_add(index)
            .max(1);
        Ok(Self {
            protocol,
            passthrough,
            image_id,
        })
    }

    pub(crate) fn cell_size(&self) -> (u16, u16) {
        let size = crossterm::terminal::window_size().ok();
        let width = size
            .as_ref()
            .and_then(|size| size.width.checked_div(size.columns))
            .filter(|width| *width > 0)
            .unwrap_or(DEFAULT_CELL_WIDTH);
        let height = size
            .and_then(|size| size.height.checked_div(size.rows))
            .filter(|height| *height > 0)
            .unwrap_or(DEFAULT_CELL_HEIGHT);
        (width, height)
    }

    pub(crate) fn write(
        &self,
        svg: &str,
        output: &mut impl Write,
        width_cells: u16,
    ) -> anyhow::Result<()> {
        self.write_image(svg, output, width_cells, true)
    }

    pub(crate) fn clear(&self, output: &mut impl Write) -> io::Result<()> {
        if self.protocol == Protocol::Kitty {
            self.write_frame(
                output,
                &format!("\x1b_Ga=d,d=I,i={},q=2\x1b\\", self.image_id),
            )?;
        }
        Ok(())
    }

    fn write_image(
        &self,
        svg: &str,
        output: &mut impl Write,
        width_cells: u16,
        keep_cursor: bool,
    ) -> anyhow::Result<()> {
        let width_cells = width_cells.max(1);
        let png = casemap::flow_image::png(svg, IMAGE_SCALE)?;
        match self.protocol {
            Protocol::Kitty => {
                self.write_kitty(output, &STANDARD.encode(png), width_cells, keep_cursor)?
            }
            Protocol::Iterm => self.write_iterm(output, &STANDARD.encode(png), width_cells)?,
            Protocol::Sixel => {
                let (cell_width, _) = self.cell_size();
                let width_pixels = u32::from(cell_width) * u32::from(width_cells);
                self.write_frame(output, &encode_sixel(&png, width_pixels)?)?;
            }
        }
        Ok(())
    }

    fn write_kitty(
        &self,
        output: &mut impl Write,
        encoded: &str,
        width_cells: u16,
        keep_cursor: bool,
    ) -> io::Result<()> {
        let chunk_count = encoded.len().div_ceil(KITTY_CHUNK);
        let cursor_policy = u8::from(keep_cursor);
        for (index, start) in (0..encoded.len()).step_by(KITTY_CHUNK).enumerate() {
            let end = (start + KITTY_CHUNK).min(encoded.len());
            let payload = &encoded[start..end];
            let continuation = u8::from(index + 1 < chunk_count);
            let arguments = if index == 0 {
                format!(
                    "a=T,f=100,q=2,i={},c={width_cells},C={cursor_policy},m={continuation}",
                    self.image_id
                )
            } else {
                format!("m={continuation}")
            };
            self.write_frame(output, &format!("\x1b_G{arguments};{payload}\x1b\\"))?;
        }
        Ok(())
    }

    fn write_iterm(
        &self,
        output: &mut impl Write,
        encoded: &str,
        width_cells: u16,
    ) -> anyhow::Result<()> {
        let options = format!("inline=1;width={width_cells};preserveAspectRatio=1");
        if !self.passthrough {
            if encoded.len() > ITERM_SINGLE_FILE_LIMIT {
                return Err(ItermSizeLimit.into());
            }
            self.write_frame(output, &format!("\x1b]1337;File={options}:{encoded}\x07"))?;
            return Ok(());
        }
        self.write_frame(output, &format!("\x1b]1337;MultipartFile={options}\x07"))?;
        for start in (0..encoded.len()).step_by(ITERM_CHUNK) {
            let end = (start + ITERM_CHUNK).min(encoded.len());
            self.write_frame(
                output,
                &format!("\x1b]1337;FilePart={}\x07", &encoded[start..end]),
            )?;
        }
        self.write_frame(output, "\x1b]1337;FileEnd\x07")?;
        Ok(())
    }

    fn write_frame(&self, output: &mut impl Write, frame: &str) -> io::Result<()> {
        if self.passthrough {
            let escaped = frame.replace('\x1b', "\x1b\x1b");
            write!(output, "\x1bPtmux;{escaped}\x1b\\")
        } else {
            output.write_all(frame.as_bytes())
        }
    }
}

fn graphics_settings(
    stdout_is_terminal: bool,
    environment: impl Fn(&str) -> Option<String>,
    tmux_allows_passthrough: impl FnOnce() -> bool,
) -> Result<(Protocol, bool), GraphicsReason> {
    if !stdout_is_terminal {
        return Err(Cow::Borrowed("not a terminal"));
    }
    let term = environment("TERM").unwrap_or_default();
    if term == "dumb" {
        return Err(Cow::Borrowed("TERM=dumb"));
    }
    if environment("CI").is_some() {
        return Err(Cow::Borrowed("CI"));
    }
    let inside_tmux = environment("TMUX").is_some();
    let blocked_by_tmux = if inside_tmux {
        !tmux_allows_passthrough()
    } else {
        term.starts_with("tmux")
    };
    if blocked_by_tmux {
        return Err(Cow::Borrowed(
            "tmux passthrough disabled, use `set -g allow-passthrough on`",
        ));
    }
    Ok((detect_protocol(environment)?, inside_tmux))
}

fn detect_protocol(
    environment: impl Fn(&str) -> Option<String>,
) -> Result<Protocol, GraphicsReason> {
    if let Some(protocol) = environment("CASEMAP_IMAGE_PROTOCOL") {
        return match protocol.as_str() {
            "kitty" => Ok(Protocol::Kitty),
            "iterm" => Ok(Protocol::Iterm),
            "sixel" => Ok(Protocol::Sixel),
            "none" => Err(Cow::Borrowed("CASEMAP_IMAGE_PROTOCOL=none")),
            _ => Err(Cow::Owned(format!(
                "invalid CASEMAP_IMAGE_PROTOCOL {protocol:?} (expected kitty, iterm, sixel or none)"
            ))),
        };
    }
    known_protocol(environment).ok_or(Cow::Borrowed("unknown terminal"))
}

fn known_protocol(environment: impl Fn(&str) -> Option<String>) -> Option<Protocol> {
    let term = environment("TERM").unwrap_or_default();
    let program = environment("TERM_PROGRAM").unwrap_or_default();
    if matches!(program.as_str(), "Apple_Terminal" | "vscode") {
        return None;
    }
    match program.as_str() {
        "ghostty" | "cmux" | "WezTerm" => return Some(Protocol::Kitty),
        "iTerm.app" => return Some(Protocol::Iterm),
        _ => {}
    }
    if matches!(term.as_str(), "xterm-kitty" | "xterm-ghostty" | "wezterm")
        || environment("KITTY_WINDOW_ID").is_some()
        || environment("GHOSTTY_RESOURCES_DIR").is_some()
    {
        return Some(Protocol::Kitty);
    }
    if environment("ITERM_SESSION_ID").is_some()
        || environment("LC_TERMINAL").as_deref() == Some("iTerm2")
    {
        return Some(Protocol::Iterm);
    }
    if term.starts_with("foot") || term.contains("sixel") || environment("WT_SESSION").is_some() {
        return Some(Protocol::Sixel);
    }
    None
}

fn tmux_passthrough() -> bool {
    let mut tmux = Command::new("tmux");
    tmux.args(["show-options", "-Apv"]);
    if let Some(pane) = std::env::var_os("TMUX_PANE") {
        tmux.arg("-t").arg(pane);
    }
    tmux.arg("allow-passthrough").output().is_ok_and(|output| {
        output.status.success()
            && matches!(String::from_utf8_lossy(&output.stdout).trim(), "on" | "all")
    })
}

fn encode_sixel(png: &[u8], width: u32) -> anyhow::Result<String> {
    let image =
        tiny_skia::Pixmap::decode_png(png).context("could not decode flow PNG for Sixel")?;
    let scale = width as f32 / image.width() as f32;
    let height = (image.height() as f32 * scale).ceil().max(1.0) as u32;
    if u64::from(width) * u64::from(height) > SIXEL_PIXEL_LIMIT {
        return Err(anyhow!("image exceeds the Sixel size limit"));
    }
    let mut resized =
        tiny_skia::Pixmap::new(width, height).context("could not allocate terminal flow image")?;
    let drawing = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    resized.draw_pixmap(
        0,
        0,
        image.as_ref(),
        &drawing,
        tiny_skia::Transform::from_scale(scale, scale),
        None,
    );
    let mut pixels = Vec::with_capacity(resized.pixels().len() * 3);
    for pixel in resized.pixels() {
        let color = pixel.demultiply();
        pixels.extend([color.red(), color.green(), color.blue()]);
    }
    icy_sixel::sixel_string(
        &pixels,
        width as i32,
        height as i32,
        icy_sixel::PixelFormat::RGB888,
        icy_sixel::DiffusionMethod::None,
        icy_sixel::MethodForLargest::Auto,
        icy_sixel::MethodForRep::Auto,
        icy_sixel::Quality::AUTO,
    )
    .map_err(|error| anyhow!("could not encode Sixel image ({error})"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        ItermSizeLimit, Protocol, Terminal, detect_protocol, encode_sixel, graphics_settings,
        save_png_in,
    };

    const SAMPLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="20"><rect width="30" height="20" fill="#336699"/></svg>"##;

    #[test]
    fn detects_graphics_protocols_from_terminal_signals() {
        let cases = [
            (vec![("TERM", "xterm-ghostty")], Some(Protocol::Kitty)),
            (vec![("TERM_PROGRAM", "cmux")], Some(Protocol::Kitty)),
            (vec![("TERM_PROGRAM", "iTerm.app")], Some(Protocol::Iterm)),
            (
                vec![("TERM_PROGRAM", "tmux"), ("ITERM_SESSION_ID", "session")],
                Some(Protocol::Iterm),
            ),
            (
                vec![("TERM", "tmux-256color"), ("LC_TERMINAL", "iTerm2")],
                Some(Protocol::Iterm),
            ),
            (vec![("TERM", "foot")], Some(Protocol::Sixel)),
            (vec![("WT_SESSION", "1")], Some(Protocol::Sixel)),
            (vec![("TERM_PROGRAM", "Apple_Terminal")], None),
            (vec![("TERM_PROGRAM", "vscode")], None),
            (
                vec![
                    ("TERM_PROGRAM", "vscode"),
                    ("CASEMAP_IMAGE_PROTOCOL", "sixel"),
                ],
                Some(Protocol::Sixel),
            ),
        ];
        for (variables, expected) in cases {
            let environment: HashMap<_, _> = variables.into_iter().collect();
            assert_eq!(
                detect_protocol(|key| environment.get(key).map(|value| (*value).into())).ok(),
                expected
            );
        }
    }

    #[test]
    fn explains_why_terminal_graphics_are_disabled() {
        let cases = [
            (false, vec![], "not a terminal"),
            (true, vec![("TERM", "dumb")], "TERM=dumb"),
            (true, vec![("CI", "true")], "CI"),
            (
                true,
                vec![("TMUX", "session"), ("TERM", "tmux-256color")],
                "tmux passthrough disabled, use `set -g allow-passthrough on`",
            ),
            (true, vec![("TERM", "unknown")], "unknown terminal"),
        ];
        for (is_terminal, variables, reason) in cases {
            let environment: HashMap<_, _> = variables.into_iter().collect();
            let settings = graphics_settings(
                is_terminal,
                |key| environment.get(key).map(|value| (*value).into()),
                || false,
            );
            assert_eq!(settings.unwrap_err(), reason);
        }
    }

    #[test]
    fn invalid_protocol_reason_reaches_the_notice() {
        let reason = graphics_settings(
            true,
            |key| match key {
                "CASEMAP_IMAGE_PROTOCOL" => Some("invalid".into()),
                "TERM_PROGRAM" => Some("ghostty".into()),
                _ => None,
            },
            || false,
        )
        .unwrap_err();
        let mut notice = Vec::new();
        super::write_fallback(std::path::Path::new("flow.png"), &reason, &mut notice).unwrap();
        assert_eq!(
            String::from_utf8(notice).unwrap(),
            "Terminal graphics unavailable (invalid CASEMAP_IMAGE_PROTOCOL \"invalid\" (expected kitty, iterm, sixel or none)). Saved PNG to flow.png\n"
        );
    }

    #[test]
    fn kitty_write_and_clear_are_scoped_to_the_terminal_image() {
        let terminal = Terminal::kitty_for_test();
        let mut output = Vec::new();
        terminal.write(SAMPLE, &mut output, 17).unwrap();
        terminal.clear(&mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("\x1b_Ga=T,f=100,q=2,i=42,c=17,C=1,m=0;"));
        assert!(output.ends_with("\x1b_Ga=d,d=I,i=42,q=2\x1b\\"));
    }

    #[test]
    fn kitty_transfers_large_payloads_in_ordered_chunks() {
        let terminal = Terminal::kitty_for_test();
        let first_chunk = "A".repeat(4_096);
        let encoded = format!("{first_chunk}BEEF");
        let mut output = Vec::new();
        terminal
            .write_kitty(&mut output, &encoded, 12, false)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            format!(
                "\x1b_Ga=T,f=100,q=2,i=42,c=12,C=0,m=1;{first_chunk}\x1b\\\x1b_Gm=0;BEEF\x1b\\"
            )
        );
    }

    #[test]
    fn tmux_wraps_and_doubles_the_graphics_escape_sequences() {
        let terminal = Terminal {
            protocol: Protocol::Kitty,
            passthrough: true,
            image_id: 42,
        };
        let mut output = Vec::new();
        terminal.clear(&mut output).unwrap();
        assert_eq!(
            output,
            b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=42,q=2\x1b\x1b\\\x1b\\"
        );
    }

    #[test]
    fn iterm_writes_one_inline_file_at_the_requested_cell_width() {
        let terminal = Terminal {
            protocol: Protocol::Iterm,
            passthrough: false,
            image_id: 42,
        };
        let mut output = Vec::new();
        terminal.write(SAMPLE, &mut output, 12).unwrap();
        assert!(output.starts_with(b"\x1b]1337;File=inline=1;width=12;preserveAspectRatio=1:"));
        assert!(output.ends_with(b"\x07"));
    }

    #[test]
    fn iterm_writes_single_file_for_larger_images_outside_tmux() {
        let terminal = Terminal {
            protocol: Protocol::Iterm,
            passthrough: false,
            image_id: 42,
        };
        let encoded = "A".repeat(4 * 1024 * 1024);
        let mut output = Vec::new();
        terminal.write_iterm(&mut output, &encoded, 12).unwrap();
        assert!(output.starts_with(b"\x1b]1337;File="));
        assert!(output.ends_with(b"\x07"));

        output.clear();
        let encoded = encoded + "AAAA";
        let error = terminal.write_iterm(&mut output, &encoded, 12).unwrap_err();
        assert!(error.is::<ItermSizeLimit>());
        assert!(output.is_empty());
    }

    #[test]
    fn iterm_wraps_multipart_files_for_tmux() {
        let terminal = Terminal {
            protocol: Protocol::Iterm,
            passthrough: true,
            image_id: 42,
        };
        let mut output = Vec::new();
        terminal.write_iterm(&mut output, "AAAA", 12).unwrap();
        assert!(output.starts_with(b"\x1bPtmux;\x1b\x1b]1337;MultipartFile="));
        assert!(output.ends_with(b"\x1bPtmux;\x1b\x1b]1337;FileEnd\x07\x1b\\"));
    }

    #[cfg(unix)]
    #[test]
    fn fallback_retains_private_file_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let path = save_png_in(
            SAMPLE,
            &directory.path().join("checkout.md"),
            false,
            directory.path(),
        )
        .unwrap();
        let permissions = std::fs::metadata(path).unwrap().permissions();
        assert_eq!(permissions.mode() & 0o077, 0);
    }

    #[cfg(unix)]
    #[test]
    fn fallback_directory_repairs_loose_permissions() {
        use std::fs::{self, Permissions};
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let directory = super::fallback_directory(root.path()).unwrap();
        fs::set_permissions(&directory, Permissions::from_mode(0o755)).unwrap();

        assert_eq!(super::fallback_directory(root.path()).unwrap(), directory);
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn fallback_directory_rejects_a_symlink_or_file() {
        use std::fs;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let directory = super::fallback_directory(root.path()).unwrap();
        fs::remove_dir(&directory).unwrap();
        symlink(target.path(), &directory).unwrap();

        assert!(super::fallback_directory(root.path()).is_err());
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);

        fs::remove_file(&directory).unwrap();
        fs::write(&directory, b"not a directory").unwrap();
        assert!(super::fallback_directory(root.path()).is_err());
        assert_eq!(fs::read(directory).unwrap(), b"not a directory");
    }

    #[test]
    fn fallback_writes_distinct_persistent_pngs() {
        let directory = tempfile::tempdir().unwrap();
        let first_flow = directory.path().join("first/checkout.md");
        let second_flow = directory.path().join("second/checkout.md");
        let initial = save_png_in(SAMPLE, &first_flow, false, directory.path()).unwrap();
        let other = save_png_in(SAMPLE, &second_flow, false, directory.path()).unwrap();
        assert_ne!(other, initial);
        assert!(
            initial
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("checkout-")
        );
        let initial_bytes = std::fs::read(&initial).unwrap();
        assert!(initial_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

        let updated_svg = SAMPLE.replace("#336699", "#993366");
        let updated = save_png_in(&updated_svg, &first_flow, false, directory.path()).unwrap();
        assert_eq!(updated, initial);
        assert_ne!(std::fs::read(&updated).unwrap(), initial_bytes);
        assert_eq!(std::fs::read(other).unwrap(), initial_bytes);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let parent = initial.parent().unwrap();
            assert_eq!(
                std::fs::metadata(parent).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn fallback_keeps_the_hash_when_a_long_stem_has_to_be_shortened() {
        let directory = tempfile::tempdir().unwrap();
        let stem = "é".repeat(125);
        let flow = directory.path().join(format!("{stem}.md"));
        let path = save_png_in(SAMPLE, &flow, true, directory.path()).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let (prefix, hash) = name.strip_suffix(".png").unwrap().rsplit_once('-').unwrap();
        assert!(prefix.ends_with("-before-after"));
        assert!(name.len() <= super::MAX_PNG_FILENAME_BYTES);
        assert_eq!(hash.len(), 8);
        assert!(std::fs::read(path).unwrap().starts_with(b"\x89PNG"));
    }

    #[test]
    fn sixel_writes_a_decodable_image() {
        let png = casemap::flow_image::png(SAMPLE, 2.0).unwrap();
        let output = encode_sixel(&png, 30).unwrap();
        assert!(output.contains("\"1;1;30;20"));
        let (pixels, width, height) = icy_sixel::sixel_decode(output.as_bytes()).unwrap();
        assert_eq!((width, height), (30, 24));
        assert_eq!(pixels.len(), 30 * 24 * 4);
    }
}
