use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::ensure;
use casemap::flow_image::{self, ImageOptions, Theme};
use ratatui::crossterm::{cursor, queue};
use ratatui::layout::{Margin, Rect};

use crate::image_terminal::{self, GraphicsReason, Terminal};

use super::app::{App, Entry, FIT_ZOOM, Focus, Offset, Standing, ViewMode};
use super::layout;

pub(super) struct ImageLayer {
    catalog: PathBuf,
    terminal: Result<Terminal, GraphicsReason>,
    source: Option<(ImageKey, String)>,
    frame: Option<FrameImage>,
    fallback: Option<Result<PathBuf, String>>,
    oversized: Option<FlowImageKey>,
    painted: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct FlowImageKey {
    path: String,
    revision: u64,
    mode: ViewMode,
}

#[derive(PartialEq, Eq)]
struct ImageKey {
    flow: FlowImageKey,
    selected: Option<usize>,
}

#[derive(PartialEq, Eq)]
struct FrameKey {
    area: Rect,
    zoom: u16,
    pan: Offset,
}

struct FrameImage {
    key: FrameKey,
    svg: String,
    failure: Option<String>,
}

#[derive(PartialEq, Eq)]
pub(super) enum PaintOutcome {
    Complete,
    NeedsRedraw,
}

impl ImageLayer {
    pub(super) fn new(catalog: &Path) -> Self {
        Self {
            catalog: catalog.to_owned(),
            terminal: Terminal::detect(),
            source: None,
            frame: None,
            fallback: None,
            oversized: None,
            painted: false,
        }
    }

    pub(super) fn prepare(&mut self, app: &mut App, theme: &Theme) -> bool {
        let area = layout::layout(app.area, app).diagram.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        if app.focus != Focus::Diagram || app.help || area.is_empty() {
            return self.hide();
        }
        app.image_notice = None;
        let Some(entry) = app.selected() else {
            return self.hide();
        };
        let flow = FlowImageKey {
            path: entry.flow.path.clone(),
            revision: app.revision,
            mode: app.view_mode,
        };
        let cell_size = self.inline_cell_size(&flow);
        let selected = if cell_size.is_ok() && app.view_mode != ViewMode::SideBySide {
            app.selected_step().map(|step| step.node_index)
        } else {
            None
        };
        let key = ImageKey { flow, selected };
        let Some(changed) = self.source_changed(app, entry, theme, key) else {
            return self.hide();
        };
        match cell_size {
            Ok(cell_size) => self.next_frame(app, area, cell_size, changed),
            Err(reason) => {
                self.show_fallback(app, &reason);
                self.hide()
            }
        }
    }

    fn inline_cell_size(&self, flow: &FlowImageKey) -> Result<(u16, u16), GraphicsReason> {
        if self.oversized.as_ref() == Some(flow) {
            return Err(image_terminal::ITERM_LIMIT_REASON.into());
        }
        self.terminal
            .as_ref()
            .map(Terminal::cell_size)
            .map_err(Clone::clone)
    }

    pub(super) fn hide(&mut self) -> bool {
        self.frame = None;
        self.painted
    }

    fn source_changed(
        &mut self,
        app: &App,
        entry: &Entry,
        theme: &Theme,
        key: ImageKey,
    ) -> Option<bool> {
        if self
            .source
            .as_ref()
            .is_some_and(|(previous, _)| *previous == key)
        {
            return Some(false);
        }
        let graphic = app.diagram()?.graphic.as_ref();
        let svg = match app.view_mode {
            ViewMode::SideBySide if graphic.is_some() || entry.previous.is_some() => {
                comparison_svg(entry, theme, &app.base.as_ref()?.label())
            }
            ViewMode::SideBySide => return None,
            ViewMode::Normal | ViewMode::Diff => {
                let graphic = graphic?;
                flow_image::svg(
                    &entry.flow,
                    &graphic.diagram,
                    theme,
                    ImageOptions {
                        changes: graphic.changes.as_deref(),
                        selected_step: key.selected,
                    },
                )
            }
        };
        self.source = Some((key, svg));
        self.fallback = None;
        Some(true)
    }

    fn show_fallback(&mut self, app: &mut App, reason: &str) {
        let Some((key, svg)) = &self.source else {
            return;
        };
        let saved = self.fallback.get_or_insert_with(|| {
            let path = self.catalog.join(&key.flow.path);
            image_terminal::save_png(svg, &path, key.flow.mode == ViewMode::SideBySide)
                .map_err(|failure| failure.to_string())
        });
        app.image_notice = Some(match saved {
            Ok(path) => format!(
                "Terminal image support unavailable ({reason}).\nPNG saved to {}",
                path.display()
            ),
            Err(failure) => format!(
                "Terminal image support unavailable ({reason}).\nCould not save the flow image ({failure})"
            ),
        });
    }

    fn next_frame(
        &mut self,
        app: &mut App,
        area: Rect,
        (cell_width, cell_height): (u16, u16),
        source_changed: bool,
    ) -> bool {
        let Some((_, svg)) = &self.source else {
            return false;
        };
        let viewport = match Viewport::new(svg, area, cell_width, cell_height, app.zoom()) {
            Ok(viewport) => viewport,
            Err(error) => {
                app.image_notice = Some(format!("Could not display the flow image ({error})"));
                return self.hide();
            }
        };
        if app.reveal_step
            && app.view_mode != ViewMode::SideBySide
            && let Some(step) = app.selected_step()
        {
            app.pan.rows = viewport.reveal(app.pan.rows, step.top, step.bottom);
        }
        app.reveal_step = false;
        app.set_image_extent(viewport.extent());
        let key = FrameKey {
            area,
            zoom: app.zoom(),
            pan: app.pan,
        };
        if !source_changed
            && let Some(frame) = &self.frame
            && frame.key == key
        {
            app.image_notice.clone_from(&frame.failure);
            return false;
        }
        let svg = viewport.crop(svg, app.pan);
        self.frame = Some(FrameImage {
            key,
            svg,
            failure: None,
        });
        true
    }

    pub(super) fn clear(&mut self) -> anyhow::Result<()> {
        if self.painted
            && let Ok(terminal) = &self.terminal
        {
            let mut output = io::stdout().lock();
            terminal.clear(&mut output)?;
            output.flush()?;
        }
        self.painted = false;
        Ok(())
    }

    pub(super) fn paint(&mut self, changed: bool) -> anyhow::Result<PaintOutcome> {
        if !changed {
            return Ok(PaintOutcome::Complete);
        }
        self.paint_changed_frame(&mut io::stdout().lock())
    }

    fn paint_changed_frame(&mut self, output: &mut impl Write) -> anyhow::Result<PaintOutcome> {
        if let (Ok(terminal), Some(frame)) = (&self.terminal, &mut self.frame) {
            let area = frame.key.area;
            queue!(output, cursor::MoveTo(area.x, area.y))?;
            if let Err(error) = terminal.write(&frame.svg, output, area.width) {
                if error.is::<image_terminal::ItermSizeLimit>() {
                    self.oversized = self.source.as_ref().map(|(key, _)| key.flow.clone());
                    return Ok(PaintOutcome::NeedsRedraw);
                }
                if error.downcast_ref::<io::Error>().is_some() {
                    return Err(error);
                }
                frame.failure = Some(format!("Could not display the flow image ({error})"));
                return Ok(PaintOutcome::NeedsRedraw);
            }
            output.flush()?;
            self.painted = true;
        }
        Ok(PaintOutcome::Complete)
    }

    pub(super) fn prepare_redraw(&mut self, app: &mut App, theme: &Theme) {
        if let Some(failure) = self.frame.as_ref().and_then(|frame| frame.failure.as_ref()) {
            app.image_notice = Some(failure.clone());
            return;
        }
        self.prepare(app, theme);
    }
}

fn comparison_svg(entry: &Entry, theme: &Theme, base_label: &str) -> String {
    let before = entry
        .previous
        .as_ref()
        .map(|(flow, diagram)| (flow, diagram));
    let after = if matches!(entry.standing, Standing::Removed) {
        None
    } else {
        entry
            .normal
            .graphic
            .as_ref()
            .map(|graphic| (&entry.flow, &graphic.diagram))
    };
    flow_image::side_by_side_svg(before, after, theme, base_label)
}

struct Viewport {
    area: Rect,
    width: f64,
    height: f64,
    scale: f64,
    cell_width: f64,
    cell_height: f64,
}

impl Viewport {
    fn new(
        svg: &str,
        area: Rect,
        cell_width: u16,
        cell_height: u16,
        zoom: u16,
    ) -> anyhow::Result<Self> {
        let (width, height) = flow_image::size(svg);
        ensure!(width > 0.0 && height > 0.0, "missing flow image size");
        let cell_width = f64::from(cell_width.max(1));
        let cell_height = f64::from(cell_height.max(1));
        let scale =
            f64::from(area.width) * cell_width / width * f64::from(zoom) / f64::from(FIT_ZOOM);
        Ok(Self {
            area,
            width,
            height,
            scale,
            cell_width,
            cell_height,
        })
    }

    fn extent(&self) -> Offset {
        Offset {
            rows: (self.height * self.scale / self.cell_height).ceil() as u16,
            columns: (self.width * self.scale / self.cell_width).ceil() as u16,
        }
    }

    fn reveal(&self, current: u16, top: u32, bottom: u32) -> u16 {
        let top = (f64::from(top) * self.scale / self.cell_height).floor() as u16;
        let bottom = (f64::from(bottom) * self.scale / self.cell_height).ceil() as u16;
        if current > top {
            return top;
        }
        let view_end = current.saturating_add(self.area.height);
        if view_end < bottom {
            return bottom.saturating_sub(self.area.height).min(top);
        }
        current
    }

    fn crop(&self, svg: &str, pan: Offset) -> String {
        let width = (f64::from(self.area.width) * self.cell_width / self.scale).min(self.width);
        let height = (f64::from(self.area.height) * self.cell_height / self.scale).min(self.height);
        let left = (f64::from(pan.columns) * self.cell_width / self.scale).min(self.width - width);
        let top = (f64::from(pan.rows) * self.cell_height / self.scale).min(self.height - height);
        flow_image::crop(svg, left, top, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::app::BaseInfo;
    use casemap::Options;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    const SAMPLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="800"><title>Flow</title></svg>"#;

    #[cfg(unix)]
    #[test]
    fn paint_failure_redraw_shows_the_notice_when_the_terminal_resizes() {
        let directory = tempfile::tempdir().unwrap();
        let loaded = super::super::commerce_loaded(&Options::default());
        let mut app = App::new(loaded, false, false);
        app.resize(120, 30);
        app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let mut images = ImageLayer::new(directory.path());
        images.terminal = Ok(Terminal::kitty_for_test());
        let theme = Theme::dark();
        assert!(images.prepare(&mut app, &theme));
        images.frame.as_mut().unwrap().svg = "invalid SVG".into();
        app.resize(100, 30);

        let outcome = images.paint_changed_frame(&mut Vec::new()).unwrap();
        assert!(matches!(outcome, PaintOutcome::NeedsRedraw));
        images.prepare_redraw(&mut app, &theme);
        assert!(
            app.image_notice
                .as_deref()
                .unwrap_or_default()
                .starts_with("Could not display the flow image")
        );
        assert!(images.prepare(&mut app, &theme));
        assert!(app.image_notice.is_none());
    }

    #[test]
    fn fallback_png_is_reused_across_step_selection_and_zoom() {
        let directory = assert_fs::TempDir::new().unwrap();
        let stem = directory.path().file_name().unwrap().to_str().unwrap();
        let mut loaded = super::super::commerce_loaded(&Options::default());
        loaded.flows[0].flow.path = format!("{stem}.md");
        loaded.base = Some(BaseInfo {
            reference: "main".into(),
            sha: "1234567890".into(),
        });
        let mut app = App::new(loaded, false, false);
        app.resize(120, 30);
        app.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let mut images = ImageLayer {
            catalog: directory.path().to_owned(),
            terminal: Err("TERM=dumb".into()),
            source: None,
            frame: None,
            fallback: None,
            oversized: None,
            painted: false,
        };
        let theme = Theme::dark();
        images.prepare(&mut app, &theme);
        let path = images.fallback.as_ref().unwrap().as_ref().unwrap().clone();
        assert!(
            path.file_stem()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("{stem}-"))
        );
        assert!(app.image_notice.as_deref().unwrap().contains("TERM=dumb"));

        let flow = images.source.as_ref().unwrap().0.flow.clone();
        images.oversized = Some(flow);
        let cached_contents = b"fallback image already saved";
        std::fs::write(&path, cached_contents).unwrap();
        app.handle(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE));
        images.prepare(&mut app, &theme);
        assert_eq!(std::fs::read(&path).unwrap(), cached_contents);
        assert!(
            app.image_notice
                .as_deref()
                .unwrap()
                .contains(image_terminal::ITERM_LIMIT_REASON)
        );

        app.handle(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        images.prepare(&mut app, &theme);
        let updated_path = images.fallback.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(updated_path, &path);
        assert!(std::fs::read(&path).unwrap().starts_with(b"\x89PNG"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn fits_to_width_and_crops_the_zoomed_panel_in_terminal_cells() {
        let panel = Rect::new(2, 2, 80, 20);
        let fit = Viewport::new(SAMPLE, panel, 10, 20, 100).unwrap();
        assert_eq!(
            fit.extent(),
            Offset {
                columns: 80,
                rows: 32
            }
        );
        assert_eq!(fit.reveal(0, 620, 670), 7);
        assert!(
            fit.crop(
                SAMPLE,
                Offset {
                    columns: 0,
                    rows: 8
                }
            )
            .contains(r#"viewBox="0.00 200.00 1000.00 500.00""#)
        );

        let zoom = Viewport::new(SAMPLE, panel, 10, 20, 200).unwrap();
        assert_eq!(
            zoom.extent(),
            Offset {
                columns: 160,
                rows: 64
            }
        );
        assert!(
            zoom.crop(
                SAMPLE,
                Offset {
                    columns: 40,
                    rows: 8
                }
            )
            .contains(r#"viewBox="250.00 100.00 500.00 250.00""#)
        );
    }
}
