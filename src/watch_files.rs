use crate::cmd::cmd;
use crate::config::{Config, Settings, SettingsExt};
use crate::dirs;
use crate::toolset::Toolset;
use eyre::Result;
use globset::{GlobBuilder, GlobSetBuilder};
use itertools::Itertools;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{collections::BTreeSet, sync::Arc};

pub use mise_config::watch_files::WatchFile;

const GIT_REWROTE_VAR: &str = "MISE_WATCH_FILES_GIT_REWRITTEN";

pub(crate) static MODIFIED_FILES: Mutex<Option<BTreeSet<PathBuf>>> = Mutex::new(None);

pub(crate) fn add_modified_file(file: PathBuf) {
    let mut mu = MODIFIED_FILES.lock().unwrap();
    let set = mu.get_or_insert_with(BTreeSet::new);
    set.insert(file);
}

/// Whether git rewrote every one of `files`: each was last modified no later
/// than git last moved HEAD to another commit in `root`'s checkout. A checkout,
/// pull or rebase updates the working tree and then logs the HEAD move, while
/// an edit made afterwards is newer than the log. False outside a git checkout.
fn git_rewrote(root: &Path, files: &[&PathBuf]) -> bool {
    let Some(log) = git_dir(root).map(|d| d.join("logs").join("HEAD")) else {
        return false;
    };
    let Some(logged) = last_rewrite_time(&log) else {
        return false;
    };
    // A deleted file has no mtime to compare, so it neither counts against the
    // rest nor, alone, shows that git rewrote anything.
    let mtimes = files
        .iter()
        .filter_map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .collect_vec();
    !mtimes.is_empty() && mtimes.iter().all(|mtime| *mtime <= logged)
}

/// When git last rewrote files according to HEAD's reflog, or `None` if the
/// latest move that matters did not. See [`last_rewrite`].
fn last_rewrite_time(log: &Path) -> Option<SystemTime> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(log).ok()?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    // Read the end of the log, and further back only while it holds nothing
    // but skipped entries.
    let mut window = 4096;
    let rewrite = loop {
        let start = len.saturating_sub(window);
        let mut tail = Vec::new();
        f.seek(SeekFrom::Start(start)).ok()?;
        f.read_to_end(&mut tail).ok()?;
        let tail = String::from_utf8_lossy(&tail);
        // A window that starts inside the log may cut its first entry.
        let entries = match start {
            0 => &tail[..],
            _ => tail.split_once('\n').map_or("", |(_, rest)| rest),
        };
        match last_rewrite(entries) {
            Scan::Done(rewrite) => break rewrite?,
            Scan::Earlier if start > 0 => window *= 2,
            Scan::Earlier => return None,
        }
    };
    match rewrite {
        Rewrite::AtLogWrite => f.metadata().and_then(|m| m.modified()).ok(),
        // The entry's time has whole seconds, so allow for files written within
        // that second.
        Rewrite::At(secs) => Some(UNIX_EPOCH + Duration::from_secs(secs + 1)),
    }
}

#[derive(Debug, PartialEq)]
enum Rewrite {
    /// The latest entry: the log file's mtime is the precise time.
    AtLogWrite,
    /// An earlier entry, logged at this many seconds after the epoch.
    At(u64),
}

#[derive(Debug, PartialEq)]
enum Scan {
    /// The entries decide it: the rewrite, or `None` if git rewrote nothing.
    Done(Option<Rewrite>),
    /// Every entry was skipped, so the answer lies in earlier ones.
    Earlier,
}

/// The latest HEAD reflog entry (`<old> <new> <who> <when>\t<message>`) that
/// moved HEAD from one commit to another without committing. Entries that leave
/// HEAD on the same commit, as `git checkout -b` and `git reset` to HEAD do, or
/// that lack the old or new commit, as the pair `git branch -m` logs does,
/// rewrote nothing and are skipped. A commit keeps the files you edited, so
/// reaching one first means git did not rewrite them.
fn last_rewrite(entries: &str) -> Scan {
    let mut skipped = false;
    for entry in entries.lines().rev().filter(|l| !l.is_empty()) {
        let Some((head, msg)) = entry.split_once('\t') else {
            return Scan::Done(None);
        };
        let mut ids = head.split(' ');
        let (Some(old), Some(new)) = (ids.next(), ids.next()) else {
            return Scan::Done(None);
        };
        if msg.starts_with("commit") {
            return Scan::Done(None);
        }
        if old == new || [old, new].iter().any(|id| id.bytes().all(|b| b == b'0')) {
            skipped = true;
            continue;
        }
        if !skipped {
            return Scan::Done(Some(Rewrite::AtLogWrite));
        }
        let secs = head.rsplit(' ').nth(1).and_then(|s| s.parse().ok());
        return Scan::Done(secs.map(Rewrite::At));
    }
    Scan::Earlier
}

/// The git directory of the checkout containing `root`: a `.git` directory, or
/// the target of a `.git` file in a linked worktree. Reads no git config, so it
/// stays cheap on the prompt path.
fn git_dir(root: &Path) -> Option<PathBuf> {
    root.ancestors().find_map(|dir| {
        let dot_git = dir.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        let target = std::fs::read_to_string(&dot_git).ok()?;
        let target = Path::new(target.strip_prefix("gitdir:")?.trim());
        Some(dir.join(target))
    })
}

pub async fn execute_runs(config: &Arc<Config>, ts: &Toolset) {
    let files = {
        let mut mu = MODIFIED_FILES.lock().unwrap();
        mu.take().unwrap_or_default()
    };
    if files.is_empty() {
        return;
    }
    // watch_files runs are hooks in effect, so they follow the same gating:
    // skipped with --no-hooks / MISE_NO_HOOKS / no_hooks, and in safe mode.
    if Settings::no_hooks() || Settings::get().no_hooks.unwrap_or(false) {
        return;
    }
    if Settings::get().safe {
        debug!("skipping watch_files: safe mode (MISE_SAFE=1)");
        return;
    }
    for (root, wf) in config.watch_file_hooks().unwrap_or_default() {
        match has_matching_files(&root, &wf, &files) {
            Ok(files) if files.is_empty() => {
                continue;
            }
            Ok(files) => {
                if wf.task.is_some() && wf.run.is_some() {
                    warn!("watch_file hook has both run and task set, using task");
                }
                if wf.task.is_some() && wf.shell.is_some() {
                    warn!("watch_file hook has both shell and task set, ignoring shell");
                }
                let git_rewrote = git_rewrote(&root, &files);
                let result = if let Some(task_name) = &wf.task {
                    execute_task(config, ts, &root, task_name, files, git_rewrote).await
                } else if let Some(run) = &wf.run {
                    execute(
                        config,
                        ts,
                        &root,
                        run,
                        wf.shell.as_deref(),
                        files,
                        git_rewrote,
                    )
                    .await
                } else {
                    warn!("watch_file hook has neither run nor task set, skipping");
                    continue;
                };
                if let Err(e) = result {
                    warn!("error executing watch_file hook: {e}");
                }
            }
            Err(e) => {
                warn!("error matching files: {e}");
            }
        }
    }
}

async fn execute(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    run: &str,
    shell: Option<&str>,
    files: Vec<&PathBuf>,
    git_rewrote: bool,
) -> Result<()> {
    let modified_files_var = files
        .iter()
        .map(|f| f.to_string_lossy().replace(':', "\\:"))
        .join(":");
    let direct_enabled = shell.is_none() && Settings::get().implicit_inline_shell();
    let mut shell = match shell {
        Some(shell) => crate::path::split_shell_command(shell)?,
        None => Settings::get().default_inline_shell()?,
    };
    Settings::get().maybe_no_profile(&mut shell);
    let (program, shell_args) = shell.split_first().ok_or_else(|| {
        eyre::eyre!(
            "inline shell is empty; check watch_files.shell or unix_default_inline_shell_args / windows_default_inline_shell_args"
        )
    })?;

    let args = shell_args
        .iter()
        .map(|s| s.as_str())
        .chain(once(run))
        .collect_vec();
    let mut env = ts.full_env(config).await?;
    env.insert("MISE_WATCH_FILES_MODIFIED".to_string(), modified_files_var);
    env.remove(GIT_REWROTE_VAR);
    if git_rewrote {
        env.insert(GIT_REWROTE_VAR.to_string(), "1".to_string());
    }
    env.insert(
        crate::secrets::DENIED_MARKER.to_string(),
        "watch_files".to_string(),
    );
    if let Some(cwd) = &*dirs::CWD {
        env.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    env.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    // TODO: this should be different but I don't have easy access to it
    // env.insert("MISE_CONFIG_ROOT".to_string(), root.to_string_lossy().to_string());
    // On Windows, `cmd /c <run>` must receive the command verbatim so inner
    // double quotes survive (#9355). Mirror the hook path: spawn a raw Command
    // with stdout redirected to our stderr handle (duct's stdout_to_stderr) and
    // the full env. Non-cmd shells / Unix fall through to the duct path below.
    #[cfg(windows)]
    {
        if let Some(mut c) = crate::path::cmd_verbatim_command(program, shell_args, run) {
            use std::os::windows::io::AsHandle;
            c.env_clear();
            c.envs(env.iter());
            c.stdout(std::io::stderr().as_handle().try_clone_to_owned()?);
            let status = c.status()?;
            if !status.success() {
                eyre::bail!("watch_files command failed: {status}");
            }
            return Ok(());
        }
    }
    crate::inline_command::optimize_expression(
        cmd(program, args).full_env(&env),
        run,
        &env,
        None,
        direct_enabled,
    )
    .stdout_to_stderr()
    .run()?;
    Ok(())
}

async fn execute_task(
    config: &Arc<Config>,
    ts: &Toolset,
    root: &Path,
    task_name: &str,
    files: Vec<&PathBuf>,
    git_rewrote: bool,
) -> Result<()> {
    let modified_files_var = files
        .iter()
        .map(|f| f.to_string_lossy().replace(':', "\\:"))
        .join(":");
    let mise_bin = crate::shims::running_mise_bin();

    let mut env = ts.full_env(config).await?;
    env.insert("MISE_WATCH_FILES_MODIFIED".to_string(), modified_files_var);
    env.remove(GIT_REWROTE_VAR);
    if git_rewrote {
        env.insert(GIT_REWROTE_VAR.to_string(), "1".to_string());
    }
    env.insert("MISE_NO_HOOKS".to_string(), "1".to_string());
    env.insert(
        crate::secrets::DENIED_MARKER.to_string(),
        "watch_files".to_string(),
    );
    if let Some(cwd) = &*dirs::CWD {
        env.insert(
            "MISE_ORIGINAL_CWD".to_string(),
            cwd.to_string_lossy().to_string(),
        );
    }
    env.insert(
        "MISE_PROJECT_ROOT".to_string(),
        root.to_string_lossy().to_string(),
    );
    cmd(
        mise_bin,
        ["--cd", &root.to_string_lossy(), "run", task_name],
    )
    .stdout_to_stderr()
    .full_env(env)
    .run()?;
    Ok(())
}

fn has_matching_files<'a>(
    root: &Path,
    wf: &'a WatchFile,
    files: &'a BTreeSet<PathBuf>,
) -> Result<Vec<&'a PathBuf>> {
    let mut glob = GlobSetBuilder::new();
    for pattern in &wf.patterns {
        match GlobBuilder::new(pattern).literal_separator(true).build() {
            Ok(g) => {
                glob.add(g);
            }
            Err(e) => {
                warn!("invalid glob pattern: {e}");
            }
        }
    }
    let glob = glob.build()?;
    Ok(files
        .iter()
        .filter(|file| {
            if let Ok(rel) = file.strip_prefix(root) {
                !glob.matches(rel).is_empty()
            } else {
                false
            }
        })
        .collect())
}

pub(crate) fn glob(root: &Path, patterns: &[String]) -> Result<Vec<PathBuf>> {
    if patterns.is_empty() {
        return Ok(vec![]);
    }
    let opts = glob::MatchOptions {
        require_literal_separator: true,
        ..Default::default()
    };
    Ok(patterns
        .iter()
        .map(|pattern| root.join(pattern).to_string_lossy().to_string())
        .filter_map(|pattern| glob::glob_with(&pattern, opts).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|paths| paths.filter_map(|p| p.ok()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Rewrite, Scan, last_rewrite, last_rewrite_time};
    use std::time::{Duration, UNIX_EPOCH};

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";
    const ZERO: &str = "0000000000000000000000000000000000000000";

    fn entry(old: &str, new: &str, secs: u64, msg: &str) -> String {
        format!("{old} {new} t <t@example.com> {secs} +0000\t{msg}\n")
    }

    #[test]
    fn a_move_to_another_commit_rewrote_files() {
        let log = entry(A, B, 100, "checkout: moving from feat to main");
        assert_eq!(last_rewrite(&log), Scan::Done(Some(Rewrite::AtLogWrite)));
        let log = entry(A, B, 100, "pull: Fast-forward");
        assert_eq!(last_rewrite(&log), Scan::Done(Some(Rewrite::AtLogWrite)));
    }

    #[test]
    fn a_commit_did_not_rewrite_files() {
        let log = entry(A, B, 100, "checkout: moving from feat to main")
            + &entry(B, A, 200, "commit: two");
        assert_eq!(last_rewrite(&log), Scan::Done(None));
        assert_eq!(
            last_rewrite(&entry(ZERO, A, 100, "commit (initial): one")),
            Scan::Done(None)
        );
    }

    #[test]
    fn staying_on_the_same_commit_did_not_rewrite_files() {
        let log = entry(A, B, 100, "commit: two")
            + &entry(B, B, 200, "checkout: moving from main to feat")
            + &entry(B, B, 300, "reset: moving to HEAD")
            + &entry(
                ZERO,
                B,
                400,
                "Branch: renamed refs/heads/feat to refs/heads/feat2",
            );
        assert_eq!(last_rewrite(&log), Scan::Done(None));
    }

    #[test]
    fn a_move_before_staying_on_the_same_commit_rewrote_files_then() {
        let log = entry(A, B, 100, "checkout: moving from main to other")
            + &entry(B, B, 200, "checkout: moving from other to feature");
        assert_eq!(last_rewrite(&log), Scan::Done(Some(Rewrite::At(100))));
    }

    #[test]
    fn a_branch_rename_is_skipped() {
        let rename = "Branch: renamed refs/heads/feat to refs/heads/feat2";
        let log = entry(A, B, 100, "checkout: moving from main to feat")
            + &entry(B, ZERO, 400, rename)
            + &entry(ZERO, B, 400, rename);
        assert_eq!(last_rewrite(&log), Scan::Done(Some(Rewrite::At(100))));
    }

    #[test]
    fn a_malformed_entry_did_not_rewrite_files() {
        assert_eq!(last_rewrite("garbage"), Scan::Done(None));
    }

    #[test]
    fn only_skipped_entries_need_earlier_ones() {
        assert_eq!(last_rewrite(""), Scan::Earlier);
        let log = entry(B, B, 200, "checkout: moving from other to feature");
        assert_eq!(last_rewrite(&log), Scan::Earlier);
    }

    #[test]
    fn a_move_far_back_in_the_log_is_found() {
        let mut log = entry(A, B, 100, "checkout: moving from main to other");
        for i in 0..100 {
            log += &entry(
                B,
                B,
                200,
                &format!("checkout: moving from b{i} to b{}", i + 1),
            );
        }
        assert!(log.len() > 4096 * 2);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("HEAD");
        std::fs::write(&path, log).unwrap();
        assert_eq!(
            last_rewrite_time(&path),
            Some(UNIX_EPOCH + Duration::from_secs(101))
        );
    }
}
