use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;

use anyhow::{Context, anyhow, bail};
use casemap::anchors::{self, Target};

use crate::LazyCodeIndex;

use super::app::{App, Focus, Standing};

const PREVIEW_BYTES: u64 = 8192;
const PREVIEW_LINES: usize = 12;

pub(super) struct CodePanel {
    index: LazyCodeIndex,
    preview_key: Option<CodeKey>,
    path: Option<PathBuf>,
    open_sender: mpsc::Sender<OpenOutcome>,
    open_receiver: mpsc::Receiver<OpenOutcome>,
}

#[derive(PartialEq, Eq)]
struct CodeKey {
    path: String,
    revision: u64,
    node: String,
    removed: bool,
}

struct OpenOutcome {
    program: String,
    status: io::Result<ExitStatus>,
}

impl OpenOutcome {
    fn failure(self) -> Option<String> {
        let program = self.program;
        match self.status {
            Ok(status) if status.success() => None,
            Ok(status) => Some(match status.code() {
                Some(code) => format!("{program} exited with status {code}"),
                None => format!("{program} failed: {status}"),
            }),
            Err(error) => Some(program_failure(&program, &error)),
        }
    }
}

fn program_failure(program: &str, error: &io::Error) -> String {
    format!("{program} failed: {error}")
}

impl CodePanel {
    pub(super) fn new(catalog: &Path) -> Self {
        let (open_sender, open_receiver) = mpsc::channel();
        Self {
            index: LazyCodeIndex::deferred(catalog),
            preview_key: None,
            path: None,
            open_sender,
            open_receiver,
        }
    }

    pub(super) fn refresh(&mut self, app: &mut App) {
        if app.focus != Focus::Diagram {
            return;
        }
        let Some((key, flow)) = self.selection(app) else {
            app.source_preview.clear();
            self.preview_key = None;
            self.path = None;
            return;
        };
        if self.preview_key.as_ref() == Some(&key) {
            return;
        }
        if self
            .preview_key
            .as_ref()
            .is_none_or(|previous| previous.revision != key.revision)
        {
            self.index.files = None;
        }
        match self.lookup(flow, &key.node) {
            Ok(Some((path, preview))) => {
                app.source_preview = preview;
                self.path = Some(path);
            }
            Ok(None) => {
                app.source_preview = vec!["No anchored code".into()];
                self.path = None;
            }
            Err(error) => {
                app.source_preview = vec![format!("Could not read anchored code ({error})")];
                self.path = None;
            }
        }
        self.preview_key = Some(key);
    }

    fn selection<'a>(&self, app: &'a App) -> Option<(CodeKey, &'a casemap::flows::Flow)> {
        let entry = app.selected()?;
        let graphic = app.diagram()?.graphic.as_ref()?;
        let node = &graphic.diagram.nodes[app.selected_step()?.node_index];
        let removed =
            node.mark == casemap::Mark::Removed || matches!(entry.standing, Standing::Removed);
        let flow = if removed {
            entry
                .previous
                .as_ref()
                .map_or(&entry.flow, |(flow, _)| flow)
        } else {
            &entry.flow
        };
        Some((
            CodeKey {
                path: entry.flow.path.clone(),
                revision: app.revision,
                node: node.id.clone(),
                removed,
            },
            flow,
        ))
    }

    fn lookup(
        &mut self,
        flow: &casemap::flows::Flow,
        node: &str,
    ) -> anyhow::Result<Option<(PathBuf, Vec<String>)>> {
        let (anchors, _) = anchors::flow_anchors(flow);
        let step = Target::Step(node.into());
        let assignment = anchors.iter().filter(|anchor| anchor.target == step);
        let shared = anchors
            .iter()
            .filter(|anchor| anchor.target == Target::Flow);
        let patterns = assignment
            .chain(shared)
            .flat_map(|anchor| anchor.patterns.iter())
            .collect::<Vec<_>>();
        if patterns.is_empty() {
            return Ok(None);
        }
        let root = self.index.root()?.canonicalize()?;
        let files = self.index.files()?;
        for pattern in patterns {
            for relative in files
                .iter()
                .filter(|file| anchors::pattern_matches(pattern, file))
            {
                let Some(path) = local_file(&root, relative) else {
                    continue;
                };
                let mut bytes = Vec::new();
                File::open(&path)?
                    .take(PREVIEW_BYTES)
                    .read_to_end(&mut bytes)?;
                let content = String::from_utf8_lossy(&bytes);
                let mut preview = vec![relative.clone()];
                preview.extend(content.lines().take(PREVIEW_LINES).map(|line| {
                    line.chars()
                        .filter(|character| !character.is_control() || *character == '\t')
                        .collect::<String>()
                        .replace('\t', "    ")
                }));
                return Ok(Some((path, preview)));
            }
        }
        Ok(None)
    }

    fn path_to_open(&mut self) -> anyhow::Result<PathBuf> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| anyhow!("No anchored code for this step"))?;
        let root = self.index.root()?.canonicalize()?;
        let relative = path
            .strip_prefix(&root)
            .context("Anchored code is outside the repository")?;
        local_file(&root, relative).context("Anchored code is unavailable inside the repository")
    }

    pub(super) fn open(&mut self) -> anyhow::Result<Option<Editor>> {
        let path = self.path_to_open()?;
        if let Some((command, program)) = editor_command() {
            return Ok(Some(Editor {
                command,
                program,
                path,
            }));
        }
        let Some((program, arguments)) = open_program() else {
            bail!("Opening anchored code is unavailable on this platform");
        };
        self.open_in_background(path, program, arguments)?;
        Ok(None)
    }

    fn open_in_background(
        &self,
        path: PathBuf,
        program: &str,
        arguments: &[&str],
    ) -> anyhow::Result<()> {
        let mut child = Command::new(program)
            .args(arguments)
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| anyhow!(program_failure(program, &error)))?;
        let sender = self.open_sender.clone();
        let program = program.to_owned();
        std::thread::spawn(move || {
            let status = child.wait();
            let _ = sender.send(OpenOutcome { program, status });
        });
        Ok(())
    }

    pub(super) fn open_failure(&self) -> Option<String> {
        self.open_receiver
            .try_iter()
            .filter_map(OpenOutcome::failure)
            .last()
    }
}

pub(super) struct Editor {
    command: Command,
    program: String,
    path: PathBuf,
}

impl Editor {
    pub(super) fn run(self) -> io::Result<(ratatui::DefaultTerminal, Option<String>)> {
        self.run_with_terminal(ratatui::restore, ratatui::try_init)
    }

    fn run_with_terminal<Terminal>(
        mut self,
        suspend: impl FnOnce(),
        resume: impl FnOnce() -> io::Result<Terminal>,
    ) -> io::Result<(Terminal, Option<String>)> {
        suspend();
        let status = self.command.arg(&self.path).status();
        let terminal = resume()?;
        let outcome = OpenOutcome {
            program: self.program,
            status,
        };
        Ok((terminal, outcome.failure()))
    }
}

fn editor_command() -> Option<(Command, String)> {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|name| editor_for_preference(&std::env::var(name).ok()?))
}

#[cfg(unix)]
fn editor_for_preference(preference: &str) -> Option<(Command, String)> {
    let preference = preference.trim();
    if preference.is_empty() {
        return None;
    }
    let mut command = Command::new("sh");
    command.args(["-c", &format!("{preference} \"$1\""), "casemap"]);
    Some((command, preference.to_owned()))
}

#[cfg(not(unix))]
fn editor_for_preference(preference: &str) -> Option<(Command, String)> {
    let mut parts = preference.split_whitespace();
    let program = parts.next()?;
    let mut command = Command::new(program);
    command.args(parts);
    Some((command, program.to_owned()))
}

fn local_file(root: &Path, relative: impl AsRef<Path>) -> Option<PathBuf> {
    let path = root.join(relative).canonicalize().ok()?;
    (path.starts_with(root) && path.is_file()).then_some(path)
}

fn open_program() -> Option<(&'static str, &'static [&'static str])> {
    if cfg!(target_os = "macos") {
        Some(("open", &["-t"]))
    } else if cfg!(target_os = "windows") {
        Some(("notepad", &[]))
    } else if cfg!(unix) {
        Some(("xdg-open", &[]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn a_failing_editor_resumes_the_browser_terminal() {
        use std::cell::Cell;

        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let directory = assert_fs::TempDir::new().unwrap();
        let path = directory.path().join("checkout.rs");
        let opened_file = directory.path().join("opened-file");
        std::fs::write(&path, "fn checkout() {}").unwrap();
        let mut command = Command::new("sh");
        command
            .args(["-c", "printf '%s' \"$2\" > \"$1\"; exit 7", "editor"])
            .arg(&opened_file);
        let editor = Editor {
            command,
            program: "editor".into(),
            path: path.clone(),
        };
        let suspended = Cell::new(false);
        let (mut terminal, status) = editor
            .run_with_terminal(
                || suspended.set(true),
                || {
                    assert!(suspended.get());
                    assert_eq!(
                        std::fs::read_to_string(&opened_file).unwrap(),
                        path.to_string_lossy()
                    );
                    Ok(Terminal::new(TestBackend::new(60, 20)).unwrap())
                },
            )
            .unwrap();
        assert_eq!(status, Some("editor exited with status 7".into()));
        terminal
            .draw(|frame| {
                frame.render_widget(ratatui::widgets::Paragraph::new("Flow"), frame.area());
            })
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "F");
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn open_outcome_names_the_program_and_cause_of_failure() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt as _;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt as _;

        #[cfg(unix)]
        let exit_three = ExitStatus::from_raw(3 << 8);
        #[cfg(windows)]
        let exit_three = ExitStatus::from_raw(3);
        let cases = [
            (Ok(ExitStatus::from_raw(0)), None),
            (Ok(exit_three), Some("status 3")),
            (
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "opener was not found",
                )),
                Some("opener was not found"),
            ),
            #[cfg(unix)]
            (Ok(ExitStatus::from_raw(libc::SIGTERM)), Some("signal: 15")),
        ];
        let program = "custom editor";
        for (status, cause) in cases {
            let failure = OpenOutcome {
                program: program.into(),
                status,
            }
            .failure();
            if let Some(cause) = cause {
                let message = failure.unwrap();
                assert!(message.starts_with(program), "{message}");
                assert!(message.contains(cause), "{message}");
            } else {
                assert_eq!(failure, None);
            }
        }
    }

    #[test]
    fn a_missing_opener_returns_a_status_naming_the_program() {
        let directory = tempfile::tempdir().unwrap();
        let opener = directory.path().join("missing-opener");
        let program = opener.to_str().unwrap();
        let panel = CodePanel::new(directory.path());
        let error = panel
            .open_in_background(directory.path().join("checkout.rs"), program, &[])
            .unwrap_err();
        let message = error.to_string();
        assert!(message.starts_with(program), "{message}");
        assert!(message.contains("failed:"), "{message}");
    }

    #[test]
    #[cfg(unix)]
    fn quoted_editor_value_opens_the_file() {
        let directory = assert_fs::TempDir::new().unwrap();
        let script = directory.path().join("editor script");
        let path = directory.path().join("anchored file.rs");
        let output = directory.path().join("opened file");
        std::fs::write(&script, "printf '%s' \"$2\" > \"$1\"").unwrap();
        let preference = format!("sh \"{}\" \"{}\"", script.display(), output.display());
        let (command, program) = editor_for_preference(&preference).unwrap();
        let editor = Editor {
            command,
            program,
            path: path.clone(),
        };
        let (_, status) = editor.run_with_terminal(|| {}, || Ok(())).unwrap();
        assert_eq!(status, None);
        assert_eq!(
            std::fs::read_to_string(output).unwrap(),
            path.to_string_lossy()
        );
    }

    #[test]
    fn previews_the_code_anchored_to_the_selected_step() {
        let directory = assert_fs::TempDir::new().unwrap();
        let root = directory.path();
        let catalog = root.join("catalog");
        std::fs::create_dir(&catalog).unwrap();
        let source = root.join("specific.rs");
        std::fs::write(&source, "fn checkout() {}\n").unwrap();
        std::fs::write(root.join("generic.rs"), "fn generic() {}\n").unwrap();
        let flow = casemap::flows::parse_flow(
            "checkout.md",
            "# Checkout\n\nA shopper pays.\n\n```mermaid\nflowchart TD\ncheckout[Pay]\n%% @code * generic.rs\n%% @code checkout specific.rs\n```\n",
        );
        let mut panel = CodePanel::new(&catalog);
        panel.index.location = Some((root.to_path_buf(), crate::FileSource::Walk));
        let (path, preview) = panel.lookup(&flow, "checkout").unwrap().unwrap();
        assert_eq!(path, source.canonicalize().unwrap());
        assert_eq!(preview, ["specific.rs", "fn checkout() {}"]);
    }

    #[cfg(unix)]
    #[test]
    fn open_revalidates_a_path_replaced_after_the_preview() {
        let directory = assert_fs::TempDir::new().unwrap();
        let root = directory.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let original = root.join("checkout.rs");
        std::fs::write(&original, "fn checkout() {}").unwrap();
        let outside = directory.path().join("outside.rs");
        std::fs::write(&outside, "outside").unwrap();

        let mut panel = CodePanel::new(&root);
        panel.index.location = Some((root.clone(), crate::FileSource::Walk));
        panel.path = Some(original.canonicalize().unwrap());
        assert_eq!(
            panel.path_to_open().unwrap(),
            original.canonicalize().unwrap()
        );

        std::fs::remove_file(&original).unwrap();
        std::os::unix::fs::symlink(outside, &original).unwrap();
        assert!(panel.path_to_open().is_err());
    }
}
