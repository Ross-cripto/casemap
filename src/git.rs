use std::path::Path;
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("cannot run git: {error}")]
    Spawn { error: std::io::Error },
    #[error("git {args}: {reason}")]
    Failed { args: String, reason: String },
    #[error("invalid revision {revision}")]
    InvalidRevision { revision: String },
    #[error("{revision} and HEAD have no common ancestor (shallow clone?)")]
    NoCommonAncestor { revision: String },
}

fn run_output(dir: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| GitError::Spawn { error })
}

fn output_text(output: std::process::Output, args: &[&str]) -> Result<String, GitError> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = if stderr.trim().is_empty() {
            format!("failed ({})", output.status)
        } else {
            stderr.trim().to_string()
        };
        return Err(GitError::Failed {
            args: args.join(" "),
            reason,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    output_text(run_output(dir, args)?, args)
}

pub fn merge_base(dir: &Path, rev: &str) -> Result<String, GitError> {
    if rev.starts_with('-') {
        return Err(GitError::InvalidRevision {
            revision: rev.to_string(),
        });
    }
    let args = ["merge-base", rev, "HEAD"];
    let output = run_output(dir, &args)?;
    if output.status.code() == Some(1) && String::from_utf8_lossy(&output.stderr).trim().is_empty()
    {
        return Err(GitError::NoCommonAncestor {
            revision: rev.to_string(),
        });
    }
    Ok(output_text(output, &args)?.trim().to_string())
}

/// `dir` must exist even when the catalog was absent at `commit`.
pub fn files_at(dir: &Path, commit: &str) -> Result<Vec<(String, String)>, GitError> {
    let root = run(dir, &["rev-parse", "--show-toplevel"])?;
    let root = Path::new(root.trim());
    let prefix = run(dir, &["rev-parse", "--show-prefix"])?;
    let prefix = prefix.trim();
    let pathspec = if prefix.is_empty() { "." } else { prefix };
    let pathspec = format!(":(literal){pathspec}");
    let listing = run(
        root,
        &[
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            commit,
            "--",
            &pathspec,
        ],
    )?;
    let mut files = Vec::new();
    for path in listing.split('\0').filter(|path| !path.is_empty()) {
        let Some(name) = path.strip_prefix(prefix) else {
            continue;
        };
        let is_direct_child = !name.contains('/');
        if !is_direct_child || !crate::flows::is_flow_file_name(name) {
            continue;
        }
        let content = run(root, &["show", &format!("{commit}:{prefix}{name}")])?;
        files.push((name.to_string(), content));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn rejects_option_like_revisions() {
        for rev in ["--output=x", "-", "-x"] {
            assert_eq!(
                merge_base(Path::new("."), rev).unwrap_err().to_string(),
                format!("invalid revision {rev}")
            );
        }
    }

    #[test]
    fn failed_git_output_always_has_a_reason() {
        for (stderr, expected) in [
            (" \n", "git merge-base main HEAD: failed (exit status: 1)"),
            (
                " fatal: unknown revision\n",
                "git merge-base main HEAD: fatal: unknown revision",
            ),
        ] {
            let output = std::process::Output {
                status: std::process::ExitStatus::from_raw(1 << 8),
                stdout: Vec::new(),
                stderr: stderr.as_bytes().to_vec(),
            };
            let error = output_text(output, &["merge-base", "main", "HEAD"]).unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }
}
