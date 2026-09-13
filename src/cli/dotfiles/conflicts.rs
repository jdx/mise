use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use eyre::{Result, WrapErr, bail};

use crate::file::display_path;
use crate::system::history::shadow::HistoryRepo;
use crate::system::history::sync::layout::Roots;
use crate::system::history::sync::reconcile::{Conflict, Object};
use crate::system::history::sync::run;
use crate::system::history::tracked::normalize_target;

/// Inspect the local and remote sides of sharing conflicts
///
/// By default, prints a unified diff from this machine's saved version to the
/// fetched repository version. `--difftool` opens the comparison in Git's
/// configured diff tool; when `diff.tool` is unset, Git falls back to
/// `merge.tool`. This command does not change either side or resolve a conflict.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub(crate) struct DotfilesConflicts {
    /// Only inspect these conflicted paths
    #[usage(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Open the comparison in Git's configured diff or merge tool
    #[usage(long)]
    difftool: bool,

    /// Use this Git diff tool instead of the configured default
    #[usage(long, value_name = "TOOL", requires = "difftool")]
    tool: Option<String>,
}

impl DotfilesConflicts {
    pub(crate) async fn run(self) -> Result<()> {
        if !crate::config::Settings::get().history.enabled {
            bail!("history is disabled (history.enabled = false)");
        }
        let (store, tracked, _) = super::history::open().await?;
        let Some(repo) = store.repo() else {
            bail!("inspecting conflicts requires git");
        };
        let sync_lock = run::lock(&store)?;
        let mut status = run::read_status(store.state_dir())?;
        run::refresh_with_interaction(
            &store,
            &tracked,
            &mut status,
            console::user_attended_stderr(),
        )?;
        // Keep the conflict objects stable while encrypted blobs are refreshed,
        // then release the sync lock before an interactive difftool can block.
        drop(sync_lock);
        let selected = select(&status.conflicts, &self.paths)?;
        if selected.is_empty() {
            info!("no sharing conflicts");
            return Ok(());
        }
        for (path, conflict) in selected {
            miseprintln!(
                "Conflict: {} ({})",
                display_path(&path),
                conflict.kind.describe()
            );
            if self.difftool {
                run_difftool(repo, conflict, &path, self.tool.as_deref())?;
            } else {
                print_diff(repo, conflict, &path)?;
            }
        }
        Ok(())
    }
}

fn select<'a>(
    conflicts: &'a [Conflict],
    paths: &[PathBuf],
) -> Result<Vec<(PathBuf, &'a Conflict)>> {
    let roots = Roots::current();
    let requested = paths
        .iter()
        .map(|path| normalize_target(path))
        .collect::<Vec<_>>();
    let mut selected = vec![];
    for conflict in conflicts {
        let Some(path) = roots
            .locate(&conflict.branch_path)
            .path()
            .map(Path::to_path_buf)
        else {
            continue;
        };
        if requested.is_empty() || requested.contains(&path) {
            selected.push((path, conflict));
        }
    }
    for path in requested {
        if !selected.iter().any(|(selected, _)| selected == &path) {
            bail!("{} is not a sharing conflict", display_path(&path));
        }
    }
    Ok(selected)
}

fn print_diff(repo: &HistoryRepo, conflict: &Conflict, path: &Path) -> Result<()> {
    let local = side_file(repo, conflict.local.as_ref(), path, "local")?;
    let remote = side_file(repo, conflict.remote.as_ref(), path, "remote")?;
    let output = git_diff(repo, &local, &remote, path, console::colors_enabled())?;
    if !output.stdout.is_empty() {
        miseprint!("{}", String::from_utf8_lossy(&output.stdout))?;
    }
    if local.mode != remote.mode {
        miseprintln!("mode: {} -> {}", local.mode, remote.mode);
    }
    Ok(())
}

fn run_difftool(
    repo: &HistoryRepo,
    conflict: &Conflict,
    path: &Path,
    tool: Option<&str>,
) -> Result<()> {
    let local = side_file(repo, conflict.local.as_ref(), path, "local")?;
    let remote = side_file(repo, conflict.remote.as_ref(), path, "remote")?;
    let git = crate::git::plumbing_binary().ok_or_else(|| eyre::eyre!("git is unavailable"))?;
    let mut command = Command::new(git);
    crate::git::sanitize_git_command(&mut command);
    command
        .current_dir(repo.dir().parent().unwrap_or(repo.dir()))
        .args(["difftool", "--no-prompt"]);
    if let Some(tool) = tool {
        command.arg("--tool").arg(tool);
    }
    command.arg("--no-index").arg("--");
    command.arg(local.file.path()).arg(remote.file.path());
    let status = command
        .status()
        .wrap_err_with(|| format!("running Git conflict difftool for {}", display_path(path)))?;
    // Like `git diff --no-index`, difftool exits 1 when the sides differ.
    if !matches!(status.code(), Some(0 | 1)) {
        bail!("Git difftool failed ({status})");
    }
    if local.mode != remote.mode {
        miseprintln!("mode: {} -> {}", local.mode, remote.mode);
    }
    Ok(())
}

struct SideFile {
    file: tempfile::NamedTempFile,
    mode: String,
    label: String,
}

fn side_file(
    repo: &HistoryRepo,
    object: Option<&Object>,
    path: &Path,
    side: &str,
) -> Result<SideFile> {
    let suffix = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut file = tempfile::Builder::new()
        .prefix(&format!("mise-{side}-"))
        .suffix(&suffix)
        .tempfile()?;
    let mode = object
        .map_or("absent", |object| object.0.as_str())
        .to_string();
    if let Some((mode, oid)) = object {
        match mode.as_str() {
            "040000" => writeln!(file, "tree {oid}")?,
            "160000" => writeln!(file, "gitlink {oid}")?,
            _ => file.write_all(&repo.cat_object(oid)?)?,
        }
    }
    Ok(SideFile {
        file,
        mode,
        label: format!("{} ({side})", display_path(path)),
    })
}

fn git_diff(
    repo: &HistoryRepo,
    local: &SideFile,
    remote: &SideFile,
    path: &Path,
    color: bool,
) -> Result<std::process::Output> {
    let git = crate::git::plumbing_binary().ok_or_else(|| eyre::eyre!("git is unavailable"))?;
    let mut command = Command::new(git);
    crate::git::sanitize_git_command(&mut command);
    command.current_dir(repo.dir().parent().unwrap_or(repo.dir()));
    command.args([
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--patch",
        if color {
            "--color=always"
        } else {
            "--color=never"
        },
    ]);
    command.arg("--no-index").arg("--");
    command.arg(local.file.path()).arg(remote.file.path());
    let mut output = command
        .output()
        .wrap_err_with(|| format!("running Git conflict comparison for {}", display_path(path)))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        bail!(
            "Git diff failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let local_name = local.file.path().to_string_lossy();
    let remote_name = remote.file.path().to_string_lossy();
    output.stdout = String::from_utf8_lossy(&output.stdout)
        .replace(local_name.as_ref(), &local.label)
        .replace(local_name.trim_start_matches('/'), &local.label)
        .replace(remote_name.as_ref(), &remote.label)
        .replace(remote_name.trim_start_matches('/'), &remote.label)
        .into_bytes();
    Ok(output)
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap dotfiles conflicts</bold>
    $ <bold>mise bootstrap dotfiles conflicts ~/.zshrc</bold>
    $ <bold>mise bootstrap dotfiles conflicts --difftool ~/.zshrc</bold>
    $ <bold>mise bootstrap dotfiles conflicts --difftool --tool meld ~/.zshrc</bold>
"#
);
