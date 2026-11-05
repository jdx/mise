//! `mise run --at REV`: run tasks in a temporary git worktree of another revision.
//!
//! The tasks run in a child `mise`, started in a detached worktree of REV under mise's
//! temporary directory, so they load that revision's own config and tasks as if REV were
//! checked out. The worktree is registered with the user's repository, so mise's trust treats
//! it as a linked worktree: in normal mode its config shares the trust of the equivalent path in
//! the main checkout, and in paranoid mode, which binds trust to a path and its contents, it
//! has none. The child trusts nothing on its own (see [`REQUIRE_EXPLICIT_TRUST_ENV`]), so an
//! untrusted config at REV fails instead of prompting, being trusted by `--yes`, or being
//! trusted implicitly as `mise run` does in the user's own checkout.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use eyre::{Context, Result, bail, eyre};

use crate::config::Settings;
use crate::config::config_file::{self, REQUIRE_EXPLICIT_TRUST_ENV};
use crate::env;
use crate::file::display_path;
use crate::git::{Git, sanitize_git_command};
use crate::ui::ctrlc;

/// Set for the `--at-setup` command and the tasks: the repository the worktree was made from.
const SOURCE_ENV: &str = "MISE_RUN_AT_SOURCE";
/// Set for the `--at-setup` command and the tasks: the commit REV resolved to.
const REV_ENV: &str = "MISE_RUN_AT_REV";

/// Runs `child_args` (a `mise` command line without the program name) in a temporary
/// worktree of `rev`, after `setup`, and removes the worktree whatever the outcome. With
/// `dry_run`, `setup` is printed instead of run, and the child previews the tasks.
pub(crate) async fn run(
    rev: &str,
    setup: Option<&str>,
    dry_run: bool,
    child_args: Vec<String>,
) -> Result<()> {
    // The child and the tasks have this set. A `--at` there would start a worktree of its own,
    // and one that the argument stripping missed would start another, forever.
    if std::env::var_os(REV_ENV).is_some() {
        bail!("mise run --at cannot run inside another `mise run --at` ({REV_ENV} is set)");
    }
    // Ctrl-C reaches the child (it shares the terminal's process group) and the tasks stop as
    // they would without `--at`, as does git while it makes the worktree. This process waits for
    // that, then removes the worktree.
    ctrlc::exit_on_ctrl_c(false);
    let cwd = std::env::current_dir()?.canonicalize()?;
    let toplevel = git_read(&cwd, ["rev-parse", "--show-toplevel"])
        .wrap_err("mise run --at needs a git repository")?;
    let toplevel = PathBuf::from(toplevel).canonicalize()?;
    // The tasks run from the same place in the worktree as the user ran mise from in theirs.
    let subdir = cwd
        .strip_prefix(&toplevel)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let sha = Git::new(&toplevel).resolve_commit(rev)?.ok_or_else(|| {
        eyre!(
            "mise run --at: {rev} is not a commit in {}",
            display_path(&toplevel)
        )
    })?;

    let worktree = TempWorktree::add(&toplevel, &sha)?;
    info!(
        "running at {rev} ({}) in {}",
        &sha[..sha.len().min(12)],
        display_path(&worktree.path)
    );
    init_submodules(&toplevel, &worktree.path)?;
    let dir = worktree.path.join(&subdir);
    if !dir.is_dir() {
        bail!(
            "mise run --at: {} does not exist at {rev}",
            display_path(toplevel.join(&subdir))
        );
    }
    check_trust(&worktree.path, &subdir, rev)?;

    if let Some(setup) = setup.filter(|_| dry_run) {
        info!("--at-setup (not run with --dry-run): {setup}");
    } else if let Some(setup) = setup {
        let status = wait(setup_command(setup), &dir, &toplevel, &sha)
            .await
            .wrap_err("mise run --at: failed to start the --at-setup command")?;
        if !status.success() {
            bail!("mise run --at: the --at-setup command failed ({status})");
        }
    }
    let mut child = tokio::process::Command::new(&*env::MISE_BIN);
    child.args(&child_args).env(REQUIRE_EXPLICIT_TRUST_ENV, "1");
    let status = wait(child, &dir, &toplevel, &sha)
        .await
        .wrap_err("mise run --at: failed to start mise")?;
    drop(worktree);
    match exit_code(&status) {
        0 => Ok(()),
        code => Err(crate::request_exit(code)),
    }
}

/// Refuses, before anything runs, an untrusted config at REV between the worktree's root and
/// the tasks' directory. The child refuses it too: it trusts nothing on its own, so it runs no
/// task from an untrusted config, even one that would load without trust. But its error names
/// the temporary worktree, which cannot usefully be trusted; this one says what can.
fn check_trust(worktree: &Path, subdir: &Path, rev: &str) -> Result<()> {
    let dirs = subdir
        .ancestors()
        .map(|sub| worktree.join(sub))
        .collect::<Vec<_>>();
    for dir in dirs.iter().rev() {
        for file in crate::config::config_paths_in_dir(dir) {
            if !file.is_file() || config_file::is_path_trusted(&file) {
                continue;
            }
            let shown = file.strip_prefix(worktree).unwrap_or(&file).display();
            if Settings::get().paranoid.config() {
                bail!(
                    "mise run --at: {shown} at {rev} is not trusted. Paranoid mode ties trust to a \
                     config's path and contents, and the --at worktree is a new path on every run."
                );
            }
            let main = crate::git::main_checkout_equivalent(&file)
                .map(|main| config_file::config_trust_root(&main))
                .unwrap_or_else(|| config_file::config_trust_root(&file));
            bail!(
                "mise run --at: {shown} at {rev} is not trusted. Config in a --at worktree shares \
                 the trust of the repository's main checkout: review it with `git show {rev}:{shown}`, \
                 then run `mise trust {}`.",
                display_path(&main)
            );
        }
    }
    Ok(())
}

async fn wait(
    mut cmd: tokio::process::Command,
    dir: &Path,
    toplevel: &Path,
    sha: &str,
) -> std::io::Result<std::process::ExitStatus> {
    // A `GIT_DIR` inherited from a git hook would point the tasks' git at the user's checkout.
    sanitize_git_command(cmd.as_std_mut());
    cmd.current_dir(dir)
        // `--cd` was applied here already and is not in the child's arguments; the variable
        // would send the child, or a `mise` in the setup command, back to the user's checkout.
        .env_remove("MISE_CD")
        .env(SOURCE_ENV, toplevel)
        .env(REV_ENV, sha)
        .stdin(Stdio::inherit())
        // A second Ctrl-C ends this process at once; the child must not outlive the
        // worktree it runs in.
        .kill_on_drop(true);
    cmd.spawn()?.wait().await
}

fn setup_command(setup: &str) -> tokio::process::Command {
    #[cfg(unix)]
    {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", setup]);
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("cmd");
        cmd.args(["/d", "/s", "/c", setup]);
        cmd
    }
}

/// The status a shell would report: the exit code, or 128 plus the signal that ended the
/// process.
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

/// A detached worktree of one commit, removed (with `git worktree remove`) when dropped.
struct TempWorktree {
    repo: PathBuf,
    /// The directory made for this run; the worktree is a child of it.
    holder: PathBuf,
    path: PathBuf,
}

impl TempWorktree {
    fn add(repo: &Path, sha: &str) -> Result<Self> {
        let base = env::MISE_TMP_DIR.join("run-at");
        crate::file::create_dir_all(&base)?;
        let holder = tempfile::Builder::new()
            .prefix(&format!("{}-", &sha[..sha.len().min(12)]))
            .tempdir_in(&base)
            .wrap_err_with(|| format!("failed to create a directory in {}", display_path(&base)))?
            .keep();
        // Named like the checkout, for tasks and tools that look at the project directory's name.
        let name = repo.file_name().unwrap_or(OsStr::new("worktree"));
        let worktree = Self {
            repo: repo.to_path_buf(),
            path: holder.join(name),
            holder,
        };
        git_run(
            repo,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                OsStr::new("--quiet"),
                worktree.path.as_os_str(),
                OsStr::new(sha),
            ],
        )
        .wrap_err("mise run --at: git worktree add failed")?;
        Ok(worktree)
    }
}

impl Drop for TempWorktree {
    fn drop(&mut self) {
        // Twice `--force` removes a worktree with changes, initialized submodules, or a lock.
        let removed = git_run(
            &self.repo,
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                OsStr::new("--force"),
                self.path.as_os_str(),
            ],
        );
        if let Err(err) = removed {
            debug!("git worktree remove: {err:#}");
            if let Err(err) = crate::file::remove_all(&self.path) {
                warn!(
                    "mise run --at: failed to remove {}: {err:#}",
                    display_path(&self.path)
                );
            }
            let _ = git_run(&self.repo, [OsStr::new("worktree"), OsStr::new("prune")]);
        }
        let _ = crate::file::remove_all(&self.holder);
    }
}

/// `git worktree add` leaves submodules empty. Each one is cloned using the user's own clone of
/// it (`.git/modules/<name>`) as a reference, so only objects it lacks are fetched; nested
/// submodules are then initialized the usual way.
fn init_submodules(repo: &Path, worktree: &Path) -> Result<()> {
    if !worktree.join(".gitmodules").is_file() {
        return Ok(());
    }
    let common_dir = PathBuf::from(git_read(
        repo,
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let paths = git_read(
        worktree,
        [
            "config",
            "--file",
            ".gitmodules",
            "--get-regexp",
            r"^submodule\..*\.path$",
        ],
    )
    .unwrap_or_default();
    for line in paths.lines() {
        let Some((key, path)) = line.split_once(' ') else {
            continue;
        };
        let Some(name) = key
            .strip_prefix("submodule.")
            .and_then(|key| key.strip_suffix(".path"))
        else {
            continue;
        };
        let mut args = vec![
            OsStr::new("submodule").to_os_string(),
            "update".into(),
            "--init".into(),
        ];
        let reference = common_dir.join("modules").join(name);
        if reference.is_dir() {
            args.push("--reference".into());
            args.push(reference.into_os_string());
        }
        args.push("--".into());
        args.push(path.into());
        git_run(worktree, &args)
            .wrap_err_with(|| format!("mise run --at: failed to initialize submodule {path}"))?;
    }
    git_run(
        worktree,
        ["submodule", "update", "--init", "--recursive"].map(OsStr::new),
    )
    .wrap_err("mise run --at: failed to initialize nested submodules")
}

fn git_command<I, S>(dir: &Path, args: I) -> std::process::Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = std::process::Command::new("git");
    sanitize_git_command(&mut cmd);
    cmd.arg("-C").arg(dir).args(args).stdin(Stdio::null());
    cmd
}

fn git_read<I, S>(dir: &Path, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_command(dir, args).stderr(Stdio::piped()).output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Runs git with its output on stderr, where it does not mix with the tasks' stdout.
fn git_run<I, S>(dir: &Path, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git_command(dir, args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}
