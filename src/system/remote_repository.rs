//! Transfer a pinned repository without copying its configuration or credentials.
use eyre::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug)]
pub struct Source {
    _directory: tempfile::TempDir,
    pub bundle: PathBuf,
    pub revision: String,
    pub origin: String,
    /// `?ref=` named the bundled branch, so the target's checkout must be on
    /// it rather than on whatever branch it already has.
    pub requested_branch: bool,
}

/// How the target installs the transferred revision.
pub struct InstallOptions {
    pub update: bool,
    pub yes: bool,
    pub dry_run: bool,
    /// An existing checkout must be on the transferred branch; `update`
    /// switches it there.
    pub require_branch: bool,
}

fn git(path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    crate::git::sanitize_git_command(&mut command);
    let output = command.arg("-C").arg(path).args(args).output()?;
    if !output.status.success() {
        bail!("repository operation failed ({})", output.status);
    }
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches('\n')
        .to_string())
}

async fn git_async(path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    crate::git::sanitize_git_command(&mut command);
    command.arg("-C").arg(path).args(args);
    let output = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .output()
        .await?;
    if !output.status.success() {
        bail!("repository operation failed ({})", output.status);
    }
    Ok(String::from_utf8(output.stdout)?
        .trim_end_matches('\n')
        .to_string())
}

pub fn validate_origin(origin: &str) -> Result<()> {
    if origin.starts_with('-') || origin.chars().any(char::is_control) {
        bail!("invalid repository origin");
    }
    // Explicit local paths may contain colons; otherwise :: selects a Git helper.
    let explicit_local = std::path::Path::new(origin).is_absolute()
        || origin.starts_with("./")
        || origin.starts_with("../");
    if !explicit_local
        && origin.split_once("::").is_some_and(|(prefix, _)| {
            !prefix.is_empty() && !prefix.contains(['/', '\\', '[', ']', '@', ':'])
        })
    {
        bail!("Git remote helpers are not supported for remote onboarding");
    }
    if !explicit_local && origin.contains("://") {
        let url = url::Url::parse(origin).wrap_err("invalid repository URL")?;
        if !matches!(url.scheme(), "https" | "ssh" | "file") {
            bail!("remote bootstrap requires HTTPS, SSH, or a local path");
        }
    }
    if let Ok(url) = url::Url::parse(origin)
        && (url.password().is_some()
            || (url.scheme() != "ssh" && !url.username().is_empty())
            || url.query().is_some()
            || url.fragment().is_some())
    {
        bail!("repository origin must not contain credentials, query parameters, or fragments");
    }
    Ok(())
}

impl Source {
    /// Clones `origin` and bundles one branch of it: `branch` when `?ref=`
    /// named one, else the repository's default branch.
    pub async fn fetch(origin: String, branch: Option<String>) -> Result<Self> {
        let requested_branch = branch.is_some();
        validate_origin(&origin)?;
        let directory = tempfile::tempdir()?;
        let repo = directory.path().join("repo");
        let mut command = Command::new("git");
        crate::git::sanitize_git_command(&mut command);
        // No checkout: source templates and hooks are never evaluated locally.
        command
            .env("GIT_ALLOW_PROTOCOL", "https:ssh:file")
            .args(["-c", &crate::git::github_credential_config("github.com")])
            .args([
                "-c",
                &crate::git::github_credential_config("github.com:443"),
            ])
            .args(["-c", "http.followRedirects=false"])
            .args(["clone", "--no-checkout", "--no-local", "--"])
            .arg(&origin)
            .arg(&repo);
        let output = tokio::process::Command::from(command)
            .kill_on_drop(true)
            .output()
            .await?;
        if !output.status.success() {
            bail!("could not fetch setup repository using local Git authentication");
        }
        if let Some(branch) = &branch {
            // the clone has every branch of origin; without a checkout, HEAD
            // is moved to the requested one by its refs alone
            let tracking = format!("refs/remotes/origin/{branch}");
            if git_async(&repo, &["show-ref", "--verify", "--quiet", &tracking])
                .await
                .is_err()
            {
                bail!("--adopt takes a branch, and {branch:?} is not a branch of {origin}");
            }
            let head = format!("refs/heads/{branch}");
            git_async(&repo, &["update-ref", &head, &tracking]).await?;
            git_async(&repo, &["symbolic-ref", "HEAD", &head]).await?;
        }
        let revision = git_async(&repo, &["rev-parse", "HEAD"]).await?;
        let branch = git_async(&repo, &["symbolic-ref", "HEAD"]).await?;
        let bundle = directory.path().join("repository.bundle");
        git_async(
            &repo,
            &[
                "bundle",
                "create",
                bundle
                    .to_str()
                    .ok_or_else(|| eyre::eyre!("non-UTF8 staging path"))?,
                "HEAD",
                &branch,
            ],
        )
        .await?;
        Ok(Self {
            _directory: directory,
            bundle,
            revision,
            origin,
            requested_branch,
        })
    }
}

pub fn global_directory() -> PathBuf {
    crate::env::MISE_GLOBAL_CONFIG_FILE
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(*crate::dirs::CONFIG)
        .to_path_buf()
}

/// The branch of a transferred bundle whose tree carries the setup
/// repository marker (`.mise-history/format.toml`); `None` for an ordinary
/// repository.
pub fn history_branch(bundle: &Path, revision: &str) -> Result<Option<String>> {
    let temporary = tempfile::tempdir()?;
    let checkout = temporary.path().join("checkout");
    let mut command = Command::new("git");
    crate::git::sanitize_git_command(&mut command);
    let output = command
        .args(["clone", "--no-checkout", "--"])
        .arg(bundle)
        .arg(&checkout)
        .output()?;
    if !output.status.success() {
        bail!("invalid transferred repository bundle");
    }
    let marker = format!("{revision}:.mise-history/format.toml");
    if git(&checkout, &["cat-file", "-e", &marker]).is_err() {
        return Ok(None);
    }
    Ok(Some(git(&checkout, &["symbolic-ref", "--short", "HEAD"])?))
}

/// Installs the transferred revision as the global configuration checkout.
/// A dry run runs every check and says what it would do (clone, fast-forward,
/// adopt) without writing anything persistent.
pub fn install(
    bundle: &Path,
    origin: &str,
    revision: &str,
    options: &InstallOptions,
) -> Result<PathBuf> {
    install_at(bundle, origin, revision, options, &global_directory())
}

fn install_at(
    bundle: &Path,
    origin: &str,
    revision: &str,
    options: &InstallOptions,
    destination: &Path,
) -> Result<PathBuf> {
    let &InstallOptions {
        update,
        yes,
        dry_run,
        require_branch,
    } = options;
    validate_origin(origin)?;
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid pinned revision");
    }
    if destination.is_symlink() {
        bail!("global configuration directory must not be a symlink");
    }
    let parent = destination
        .parent()
        .ok_or_else(|| eyre::eyre!("missing parent directory"))?;
    if !dry_run {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = crate::lock_file::LockFile::new(destination).lock()?;
    // the checkout is renamed into place, so it is staged next to the
    // destination; a dry run never renames and leaves the parent alone
    let temporary = if dry_run {
        tempfile::tempdir()?
    } else {
        tempfile::tempdir_in(parent)?
    };
    let checkout = temporary.path().join("checkout");
    let shown = crate::file::display_path(destination);
    let mut command = Command::new("git");
    crate::git::sanitize_git_command(&mut command);
    let output = command
        .args(["clone", "--no-checkout", "--"])
        .arg(bundle)
        .arg(&checkout)
        .output()?;
    if !output.status.success() {
        bail!("invalid transferred repository bundle");
    }
    if git(&checkout, &["rev-parse", "HEAD"])? != revision {
        bail!("transferred revision mismatch");
    }
    let entries = git(&checkout, &["ls-tree", "-r", "-z", "--name-only", revision])?;
    for entry in entries.split('\0').filter(|s| !s.is_empty()) {
        let path = Path::new(entry);
        if path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || entry.split('/').any(|p| p.eq_ignore_ascii_case(".git"))
        {
            bail!("unsafe source repository path");
        }
        if entry.to_ascii_lowercase().ends_with(".local.toml") {
            bail!(
                "source contains machine-local configuration ({entry}); remove it from the repository before onboarding"
            );
        }
    }
    git(&checkout, &["remote", "set-url", "origin", origin])?;
    let branch = git(&checkout, &["symbolic-ref", "--short", "HEAD"])?;
    git(
        &checkout,
        &["-c", "core.hooksPath=/dev/null", "checkout", &branch],
    )?;
    if destination.join(".git").exists() {
        if git(destination, &["remote", "get-url", "origin"])? != origin {
            bail!("global configuration origin does not match");
        }
        if !git(
            destination,
            &["status", "--porcelain", "--untracked-files=no"],
        )?
        .is_empty()
        {
            bail!("global configuration has uncommitted changes");
        }
        // the full name: `--short` prints `heads/<name>` when a tag shares it
        let current = git(destination, &["symbolic-ref", "HEAD"])
            .ok()
            .and_then(|head| head.strip_prefix("refs/heads/").map(str::to_string));
        let on_branch = current.as_deref() == Some(branch.as_str());
        if require_branch && !on_branch && !update {
            let state = match &current {
                Some(current) => format!("on branch {current:?}"),
                None => "not on a branch".to_string(),
            };
            bail!(
                "global configuration at {shown} is {state}, not {branch:?}; add --update to switch it"
            );
        }
        if update {
            if !on_branch && !require_branch {
                bail!("global configuration branch differs from the transferred branch");
            }
            // the commit the checkout moves on from: its own, or that of the
            // local branch it switches to; a missing local branch is created
            let from = if on_branch {
                Some(git(destination, &["rev-parse", "HEAD"])?)
            } else {
                let local = format!("refs/heads/{branch}");
                git(destination, &["rev-parse", "--verify", "--quiet", &local]).ok()
            };
            if dry_run {
                if !on_branch {
                    miseprintln!("Would switch {shown} to branch {branch}");
                }
                // the bundle holds the history from the checkout's commit on,
                // so the fast-forward is checked there without fetching into
                // the destination
                match &from {
                    Some(head) if head == revision => miseprintln!(
                        "Would keep {shown} at {revision}, already the transferred revision"
                    ),
                    Some(head)
                        if git(&checkout, &["merge-base", "--is-ancestor", head, revision])
                            .is_ok() =>
                    {
                        miseprintln!("Would fast-forward {shown} from {head} to {revision}")
                    }
                    Some(_) => bail!(
                        "global configuration at {shown} cannot be fast-forwarded to the transferred revision"
                    ),
                    None => miseprintln!("Would create branch {branch} in {shown} at {revision}"),
                }
                return Ok(destination.to_path_buf());
            }
            git(
                destination,
                &[
                    "fetch",
                    "--no-tags",
                    bundle
                        .to_str()
                        .ok_or_else(|| eyre::eyre!("invalid bundle path"))?,
                    "HEAD",
                ],
            )?;
            if let Some(from) = &from {
                git(
                    destination,
                    &["merge-base", "--is-ancestor", from, revision],
                )?;
            }
            if !on_branch {
                // the local branch is fast-forwarded first, so the checkout
                // moves to the transferred revision in one step that either
                // completes or leaves HEAD where it was; an ignored local file
                // stops it instead of being replaced by one the branch tracks
                let local = format!("refs/heads/{branch}");
                let mut checkout = vec![
                    "-c",
                    "core.hooksPath=/dev/null",
                    "checkout",
                    "--no-overwrite-ignore",
                ];
                match &from {
                    Some(from) => {
                        git(destination, &["update-ref", &local, revision, from])?;
                        checkout.push(&branch);
                    }
                    None => checkout.extend(["-b", branch.as_str(), revision]),
                }
                git(destination, &checkout)?;
            }
            git(
                destination,
                &[
                    "-c",
                    "core.hooksPath=/dev/null",
                    "merge",
                    "--ff-only",
                    "--no-overwrite-ignore",
                    revision,
                ],
            )?;
            git(
                destination,
                &[
                    "update-ref",
                    &format!("refs/remotes/origin/{branch}"),
                    revision,
                ],
            )?;
            if !on_branch {
                let upstream = format!("--set-upstream-to=origin/{branch}");
                git(destination, &["branch", &upstream, &branch])?;
            }
        } else if dry_run {
            miseprintln!(
                "Would keep the existing checkout of {origin} at {shown}; --update fast-forwards it"
            );
        }
        return Ok(destination.to_path_buf());
    }
    let nonempty = destination.exists() && destination.read_dir()?.next().is_some();
    if nonempty {
        for entry in entries.split('\0').filter(|s| !s.is_empty()) {
            let mut ancestor = destination.to_path_buf();
            for component in Path::new(entry).components() {
                ancestor.push(component);
                if ancestor.is_symlink() {
                    bail!("existing symbolic link conflicts with adoption: {entry}");
                }
            }
            let existing = destination.join(entry);
            if existing.exists()
                && (checkout.join(entry).is_symlink()
                    || !existing.is_file()
                    || std::fs::read(&existing)? != std::fs::read(checkout.join(entry))?)
            {
                bail!("existing file conflicts with adoption: {entry}");
            }
        }
        if dry_run {
            let new_files = entries
                .split('\0')
                .filter(|s| !s.is_empty())
                .filter(|entry| !destination.join(entry).exists())
                .count();
            miseprintln!(
                "Would adopt {shown} as the global configuration repository ({new_files} new file(s); existing files and local overrides preserved)"
            );
            return Ok(destination.to_path_buf());
        }
        eprintln!(
            "Adopt existing global configuration at {} (preserving existing files and local overrides)",
            destination.display()
        );
        if !yes
            && !crate::ui::confirm("Adopt this directory as the global configuration repository?")?
                .is_yes()
        {
            bail!("adoption requires confirmation; review the directory and retry with --yes");
        }
        // Existing files are never replaced. Move only new files and Git metadata;
        // an interrupted adoption remains recoverable without deleting user data.
        for entry in entries.split('\0').filter(|s| !s.is_empty()) {
            let target = destination.join(entry);
            if !target.exists() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(checkout.join(entry), target)?;
            }
        }
        std::fs::rename(checkout.join(".git"), destination.join(".git"))?;
    } else if dry_run {
        miseprintln!("Would clone {origin} at {revision} into {shown}");
    } else {
        if destination.exists() {
            std::fs::remove_dir(destination)?;
        }
        std::fs::rename(checkout, destination)?;
    }
    Ok(destination.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repository() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-b", "main"]).unwrap();
        git(
            temp.path(),
            &["config", "user.email", "test@example.invalid"],
        )
        .unwrap();
        git(temp.path(), &["config", "user.name", "Test"]).unwrap();
        // Keep byte-comparison fixtures independent of Git for Windows' autocrlf.
        commit(temp.path(), ".gitattributes", "* -text\n");
        commit(temp.path(), "config.toml", "[tools]\n");
        temp
    }
    fn commit(repo: &Path, path: &str, contents: &str) {
        std::fs::write(repo.join(path), contents).unwrap();
        git(repo, &["add", path]).unwrap();
        git(
            repo,
            &["-c", "core.hooksPath=/dev/null", "commit", "-m", "test"],
        )
        .unwrap();
    }
    fn install_source(source: &Source, dest: &Path, update: bool) -> Result<PathBuf> {
        install_at(
            &source.bundle,
            &source.origin,
            &source.revision,
            &InstallOptions {
                update,
                yes: true,
                dry_run: false,
                require_branch: source.requested_branch,
            },
            dest,
        )
    }
    fn preview_source(source: &Source, dest: &Path, update: bool) -> Result<PathBuf> {
        install_at(
            &source.bundle,
            &source.origin,
            &source.revision,
            &InstallOptions {
                update,
                yes: true,
                dry_run: true,
                require_branch: source.requested_branch,
            },
            dest,
        )
    }
    #[tokio::test]
    async fn switching_branches_keeps_ignored_local_files() {
        let repo = repository();
        commit(repo.path(), ".gitignore", "notes.txt\n");
        git(repo.path(), &["switch", "-c", "notes"]).unwrap();
        std::fs::write(repo.path().join("notes.txt"), "tracked\n").unwrap();
        git(repo.path(), &["add", "-f", "notes.txt"]).unwrap();
        git(
            repo.path(),
            &["-c", "core.hooksPath=/dev/null", "commit", "-m", "test"],
        )
        .unwrap();
        git(repo.path(), &["switch", "main"]).unwrap();
        let origin: String = repo.path().to_str().unwrap().into();

        let main = Source::fetch(origin.clone(), Some("main".into()))
            .await
            .unwrap();
        let target = tempfile::tempdir().unwrap();
        let dest = target.path().join("mise");
        install_source(&main, &dest, false).unwrap();
        std::fs::write(dest.join("notes.txt"), "local\n").unwrap();

        let notes = Source::fetch(origin, Some("notes".into())).await.unwrap();
        assert!(install_source(&notes, &dest, true).is_err());
        assert_eq!(
            std::fs::read_to_string(dest.join("notes.txt")).unwrap(),
            "local\n"
        );
        assert_eq!(
            git(&dest, &["symbolic-ref", "HEAD"]).unwrap(),
            "refs/heads/main"
        );
    }
    #[tokio::test]
    async fn a_refused_switch_leaves_head_where_it_was() {
        let repo = repository();
        commit(repo.path(), ".gitignore", "notes.txt\n");
        git(repo.path(), &["branch", "notes"]).unwrap();
        let origin: String = repo.path().to_str().unwrap().into();
        let target = tempfile::tempdir().unwrap();
        let dest = target.path().join("mise");
        // the target has a local `notes` branch, then moves to main
        let notes = Source::fetch(origin.clone(), Some("notes".into()))
            .await
            .unwrap();
        install_source(&notes, &dest, false).unwrap();
        let main = Source::fetch(origin.clone(), Some("main".into()))
            .await
            .unwrap();
        install_source(&main, &dest, true).unwrap();
        // origin's `notes` then starts tracking a file the target ignores
        git(repo.path(), &["switch", "notes"]).unwrap();
        std::fs::write(repo.path().join("notes.txt"), "tracked\n").unwrap();
        git(repo.path(), &["add", "-f", "notes.txt"]).unwrap();
        git(
            repo.path(),
            &["-c", "core.hooksPath=/dev/null", "commit", "-m", "test"],
        )
        .unwrap();
        git(repo.path(), &["switch", "main"]).unwrap();
        std::fs::write(dest.join("notes.txt"), "local\n").unwrap();

        let notes = Source::fetch(origin, Some("notes".into())).await.unwrap();
        assert!(install_source(&notes, &dest, true).is_err());
        assert_eq!(
            git(&dest, &["symbolic-ref", "HEAD"]).unwrap(),
            "refs/heads/main"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("notes.txt")).unwrap(),
            "local\n"
        );
    }
    #[tokio::test]
    async fn fetches_the_requested_branch_only_when_it_is_a_branch() {
        let repo = repository();
        git(repo.path(), &["switch", "-c", "laptop"]).unwrap();
        commit(repo.path(), "laptop.toml", "[tools]\n");
        let laptop = git(repo.path(), &["rev-parse", "HEAD"]).unwrap();
        git(repo.path(), &["tag", "v1"]).unwrap();
        git(repo.path(), &["switch", "main"]).unwrap();
        let origin: String = repo.path().to_str().unwrap().into();

        let source = Source::fetch(origin.clone(), Some("laptop".into()))
            .await
            .unwrap();
        assert_eq!(source.revision, laptop);
        let target = tempfile::tempdir().unwrap();
        let dest = target.path().join("mise");
        install_source(&source, &dest, false).unwrap();
        assert_eq!(
            git(&dest, &["symbolic-ref", "--short", "HEAD"]).unwrap(),
            "laptop"
        );
        assert!(dest.join("laptop.toml").exists());

        // a checkout on another branch is refused until --update switches it
        let main = Source::fetch(origin.clone(), Some("main".into()))
            .await
            .unwrap();
        let err = install_source(&main, &dest, false).err().unwrap();
        assert!(err.to_string().contains("add --update"), "{err}");
        preview_source(&main, &dest, true).unwrap();
        assert_eq!(
            git(&dest, &["symbolic-ref", "--short", "HEAD"]).unwrap(),
            "laptop"
        );
        install_source(&main, &dest, true).unwrap();
        assert_eq!(
            git(&dest, &["symbolic-ref", "--short", "HEAD"]).unwrap(),
            "main"
        );
        assert!(!dest.join("laptop.toml").exists());
        // without ?ref=, the checkout's own branch is kept as before
        let default = Source::fetch(origin.clone(), None).await.unwrap();
        install_source(&default, &dest, false).unwrap();

        for git_ref in ["v1", "missing"] {
            let err = Source::fetch(origin.clone(), Some(git_ref.into()))
                .await
                .err()
                .unwrap();
            assert!(err.to_string().contains("is not a branch"), "{err}");
        }
    }
    #[tokio::test]
    async fn dry_run_previews_without_writing() {
        let repo = repository();
        let source = Source::fetch(repo.path().to_str().unwrap().into(), None)
            .await
            .unwrap();
        let target = tempfile::tempdir().unwrap();
        // a fresh destination, its parent missing too: neither is created
        let dest = target.path().join("missing").join("mise");
        preview_source(&source, &dest, false).unwrap();
        assert!(!dest.exists());
        assert!(!target.path().join("missing").exists());
        // an existing checkout is inspected, not moved
        let dest = target.path().join("mise");
        install_source(&source, &dest, false).unwrap();
        commit(
            repo.path(),
            "config.toml",
            "[settings]\nexperimental = true\n",
        );
        let next = Source::fetch(source.origin.clone(), None).await.unwrap();
        preview_source(&next, &dest, false).unwrap();
        preview_source(&next, &dest, true).unwrap();
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]).unwrap(), source.revision);
        // and nothing was fetched into it: the new revision is not there
        assert!(
            git(
                &dest,
                &["cat-file", "-e", &format!("{}^{{commit}}", next.revision)]
            )
            .is_err()
        );
        std::fs::write(dest.join("config.toml"), "dirty").unwrap();
        assert!(preview_source(&next, &dest, true).is_err());
        // adoption is previewed without writing git metadata; a conflict
        // still refuses
        let adopt = tempfile::tempdir().unwrap();
        std::fs::write(adopt.path().join("config.local.toml"), "private").unwrap();
        std::fs::write(adopt.path().join("config.toml"), "conflict").unwrap();
        assert!(preview_source(&source, adopt.path(), false).is_err());
        std::fs::write(adopt.path().join("config.toml"), "[tools]\n").unwrap();
        preview_source(&source, adopt.path(), false).unwrap();
        assert!(!adopt.path().join(".git").exists());
        assert!(!adopt.path().join(".gitattributes").exists());
    }
    #[tokio::test]
    async fn pinned_install_and_safe_updates() {
        let repo = repository();
        let source = Source::fetch(repo.path().to_str().unwrap().into(), None)
            .await
            .unwrap();
        let target = tempfile::tempdir().unwrap();
        let dest = target.path().join("mise");
        install_source(&source, &dest, false).unwrap();
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]).unwrap(), source.revision);
        assert_eq!(
            git(&dest, &["remote", "get-url", "origin"]).unwrap(),
            source.origin
        );
        assert_eq!(
            git(&dest, &["rev-parse", "--abbrev-ref", "@{upstream}"]).unwrap(),
            "origin/main"
        );
        std::fs::write(dest.join("config.local.toml"), "local").unwrap();
        commit(
            repo.path(),
            "config.toml",
            "[settings]\nexperimental = true\n",
        );
        let next = Source::fetch(source.origin.clone(), None).await.unwrap();
        install_source(&next, &dest, false).unwrap();
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]).unwrap(), source.revision);
        install_source(&next, &dest, true).unwrap();
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]).unwrap(), next.revision);
        assert_eq!(
            git(&dest, &["rev-parse", "@{upstream}"]).unwrap(),
            next.revision
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("config.local.toml")).unwrap(),
            "local"
        );
        std::fs::write(dest.join("config.toml"), "dirty").unwrap();
        assert!(install_source(&next, &dest, true).is_err());
    }
    #[tokio::test]
    async fn adoption_preserves_files_and_rejects_conflicts() {
        let repo = repository();
        let source = Source::fetch(repo.path().to_str().unwrap().into(), None)
            .await
            .unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(target.path().join("config.local.toml"), "private").unwrap();
        std::fs::write(target.path().join("config.toml"), "conflict").unwrap();
        assert!(install_source(&source, target.path(), false).is_err());
        assert!(!target.path().join(".git").exists());
        std::fs::write(target.path().join("config.toml"), "[tools]\n").unwrap();
        install_source(&source, target.path(), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.path().join("config.local.toml")).unwrap(),
            "private"
        );
        git(
            target.path(),
            &[
                "remote",
                "set-url",
                "origin",
                "https://example.invalid/other.git",
            ],
        )
        .unwrap();
        assert!(install_source(&source, target.path(), false).is_err());
    }
    #[tokio::test]
    async fn rejects_source_local_overrides_and_credential_urls() {
        let repo = repository();
        commit(repo.path(), "config.local.toml", "secret");
        let source = Source::fetch(repo.path().to_str().unwrap().into(), None)
            .await
            .unwrap();
        let target = tempfile::tempdir().unwrap();
        assert!(install_source(&source, &target.path().join("mise"), false).is_err());
        assert!(validate_origin("https://user:secret@github.com/jdx/mise").is_err());
        assert!(validate_origin("https://github.com/jdx/mise?token=secret").is_err());
        assert!(validate_origin("git@github.com:jdx/mise.git").is_ok());
        for origin in [
            "http://github.com/jdx/mise",
            "git://github.com/jdx/mise",
            "HTTP://example.com/repo",
            "ftp://example.com/repo",
            "ftps://example.com/repo",
            "helper://example.com/repo",
            "helper::repo",
            "custom_helper::repo",
            "https::https://example.com/repo",
        ] {
            assert!(validate_origin(origin).is_err());
        }
        for origin in [
            "https://github.com/jdx/mise",
            "ssh://git@github.com/jdx/mise.git",
            "git:repo",
            "git@[::1]:repo",
            "./local::repo",
            "file:///tmp/repo",
            "./repo",
        ] {
            assert!(validate_origin(origin).is_ok());
        }
    }

    #[tokio::test]
    async fn nested_adoption_preserves_siblings_and_rejects_local_overrides() {
        let repo = repository();
        std::fs::create_dir(repo.path().join("conf.d")).unwrap();
        commit(repo.path(), "conf.d/shared.toml", "[tools]\n");
        let target = tempfile::tempdir().unwrap();
        std::fs::create_dir(target.path().join("conf.d")).unwrap();
        std::fs::write(target.path().join("conf.d/private.local.toml"), "private").unwrap();
        let source = Source::fetch(repo.path().to_str().unwrap().into(), None)
            .await
            .unwrap();
        install_source(&source, target.path(), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.path().join("conf.d/private.local.toml")).unwrap(),
            "private"
        );
        assert!(target.path().join("conf.d/shared.toml").is_file());
        commit(repo.path(), "conf.d/config.local.toml", "secret");
        let source = Source::fetch(source.origin.clone(), None).await.unwrap();
        let fresh = tempfile::tempdir().unwrap();
        assert!(install_source(&source, &fresh.path().join("mise"), false).is_err());
        assert!(!fresh.path().join("mise").exists());
    }
}
