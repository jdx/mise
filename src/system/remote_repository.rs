//! Transfer a pinned repository without copying its configuration or credentials.
use eyre::{Result, bail};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug)]
pub(crate) struct Source {
    _directory: tempfile::TempDir,
    pub bundle: PathBuf,
    pub revision: String,
    pub origin: String,
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

pub(crate) fn validate_origin(origin: &str) -> Result<()> {
    if origin.starts_with('-') || origin.chars().any(char::is_control) {
        bail!("invalid repository origin");
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
    pub(crate) fn fetch(origin: String) -> Result<Self> {
        validate_origin(&origin)?;
        let directory = tempfile::tempdir()?;
        let repo = directory.path().join("repo");
        let mut command = Command::new("git");
        crate::git::sanitize_git_command(&mut command);
        // No checkout: source templates and hooks are never evaluated locally.
        let output = command
            .args(["clone", "--no-checkout", "--no-local", "--"])
            .arg(&origin)
            .arg(&repo)
            .output()?;
        if !output.status.success() {
            bail!("could not fetch setup repository using local Git authentication");
        }
        let revision = git(&repo, &["rev-parse", "HEAD"])?;
        let branch = git(&repo, &["symbolic-ref", "HEAD"])?;
        let bundle = directory.path().join("repository.bundle");
        git(
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
        )?;
        Ok(Self {
            _directory: directory,
            bundle,
            revision,
            origin,
        })
    }
}

pub(crate) fn global_directory() -> PathBuf {
    crate::env::MISE_GLOBAL_CONFIG_FILE
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(*crate::dirs::CONFIG)
        .to_path_buf()
}

pub(crate) fn install(
    bundle: &Path,
    origin: &str,
    revision: &str,
    update: bool,
    yes: bool,
) -> Result<PathBuf> {
    install_at(bundle, origin, revision, update, yes, &global_directory())
}

fn install_at(
    bundle: &Path,
    origin: &str,
    revision: &str,
    update: bool,
    yes: bool,
    destination: &Path,
) -> Result<PathBuf> {
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
    std::fs::create_dir_all(parent)?;
    let _lock = crate::lock_file::LockFile::new(destination).lock()?;
    let temporary = tempfile::tempdir_in(parent)?;
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
    if git(&checkout, &["rev-parse", "HEAD"])? != revision {
        bail!("transferred revision mismatch");
    }
    let entries = git(&checkout, &["ls-tree", "-rz", "--name-only", revision])?;
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
        if update {
            if git(destination, &["symbolic-ref", "--short", "HEAD"])? != branch {
                bail!("global configuration branch differs from the transferred branch");
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
            git(
                destination,
                &["merge-base", "--is-ancestor", "HEAD", revision],
            )?;
            git(
                destination,
                &[
                    "-c",
                    "core.hooksPath=/dev/null",
                    "merge",
                    "--ff-only",
                    revision,
                ],
            )?;
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
            update,
            true,
            dest,
        )
    }
    #[test]
    fn pinned_install_and_safe_updates() {
        let repo = repository();
        let source = Source::fetch(repo.path().to_str().unwrap().into()).unwrap();
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
        let next = Source::fetch(source.origin.clone()).unwrap();
        install_source(&next, &dest, false).unwrap();
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]).unwrap(), source.revision);
        install_source(&next, &dest, true).unwrap();
        assert_eq!(git(&dest, &["rev-parse", "HEAD"]).unwrap(), next.revision);
        assert_eq!(
            std::fs::read_to_string(dest.join("config.local.toml")).unwrap(),
            "local"
        );
        std::fs::write(dest.join("config.toml"), "dirty").unwrap();
        assert!(install_source(&next, &dest, true).is_err());
    }
    #[test]
    fn adoption_preserves_files_and_rejects_conflicts() {
        let repo = repository();
        let source = Source::fetch(repo.path().to_str().unwrap().into()).unwrap();
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
    #[test]
    fn rejects_source_local_overrides_and_credential_urls() {
        let repo = repository();
        commit(repo.path(), "config.local.toml", "secret");
        let source = Source::fetch(repo.path().to_str().unwrap().into()).unwrap();
        let target = tempfile::tempdir().unwrap();
        assert!(install_source(&source, &target.path().join("mise"), false).is_err());
        assert!(validate_origin("https://user:secret@github.com/jdx/mise").is_err());
        assert!(validate_origin("https://github.com/jdx/mise?token=secret").is_err());
        assert!(validate_origin("git@github.com:jdx/mise.git").is_ok());
    }
}
