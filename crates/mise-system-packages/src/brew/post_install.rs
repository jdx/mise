//! What Homebrew does after pouring a bottle: copy the `etc`/`var` files the
//! bottle ships, then run the formula's declarative `post_install_steps` from
//! the Homebrew API. Only the generic step types are implemented; the
//! tool-specific ones (`compile_gsettings_schemas`, `init_data_dir`, ...) are
//! reported and skipped, since mise never runs `brew`.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use eyre::{WrapErr, bail, eyre};
use serde_json::Value;
use walkdir::WalkDir;

use super::prefix;
use eyre::Result;

/// a step (or part of one) this implementation does not handle
#[derive(Debug)]
struct Unsupported(String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unsupported {}

fn unsupported<T>(what: impl Into<String>) -> Result<T> {
    Err(Unsupported(what.into()).into())
}

/// Copy `<keg>/.bottle/{etc,var}` into the prefix like brew's `pour` does,
/// then remove `.bottle`. A file that already exists and differs is left
/// alone and the bottle's copy lands beside it as `<name>.default`.
///
/// Returns the files it created so the caller can roll them back if a later
/// install step fails; on its own failure it rolls back before returning.
pub(super) fn install_bottle_config(keg: &Path) -> Result<Vec<PathBuf>> {
    let bottle = keg.join(".bottle");
    let mut created = Vec::new();
    if !bottle.is_dir() {
        return Ok(created);
    }
    let prefix = prefix::prefix();
    let roots = super::pour::link_roots();
    // a link in `var` can replace one an `etc` link resolves through, so the
    // links of both trees are checked again once both are in place
    let mut links = Vec::new();
    let copied = ["etc", "var"].into_iter().try_for_each(|dir| {
        let src = bottle.join(dir);
        if src.is_dir() {
            copy_tree_links(
                &src,
                &prefix.join(dir),
                true,
                &mut created,
                &roots,
                &mut links,
            )
            .wrap_err_with(|| format!("failed to install the bottle's {dir} files"))?;
        }
        Ok(())
    });
    let result = match copied {
        Ok(()) => recheck_links(links, &mut created, &roots),
        Err(err) => {
            undo_links(links);
            Err(err)
        }
    }
    .and_then(|()| mise_util::file::remove_all(&bottle));
    if let Err(err) = result {
        remove_created(&created);
        return Err(err);
    }
    Ok(created)
}

/// undo `install_bottle_config`
pub(super) fn remove_created(created: &[PathBuf]) {
    for path in created {
        let _ = fs::remove_file(path);
    }
}

/// Copy `src` into `dst`. With `keep_existing`, an existing different file is
/// preserved and the new one is written to `<file>.default`. Every file or
/// link it writes is appended to `created`.
///
/// `src` can come from a bottle, so nothing is written through a symlink at
/// or below `dst` (a `dst` that is a link is skipped), and a link in `src`
/// that would point outside every one of `roots` (the Homebrew prefix) from
/// its new place is not installed. A relative `dst` is taken from the working
/// directory, since that check resolves a link from `/`.
fn copy_tree(
    src: &Path,
    dst: &Path,
    keep_existing: bool,
    created: &mut Vec<PathBuf>,
    roots: &[PathBuf],
) -> Result<()> {
    // a later link can replace one an earlier link resolves through, so each
    // installed link is checked again once the whole tree is in place
    let mut links = Vec::new();
    if let Err(err) = copy_tree_links(src, dst, keep_existing, created, roots, &mut links) {
        undo_links(links);
        return Err(err);
    }
    recheck_links(links, created, roots)
}

/// [`copy_tree`] without the final check: each link it installs is appended
/// to `installed_links`, for [`recheck_links`] to check once everything that
/// could change where they lead is in place, or for [`undo_links`] when a
/// later step fails.
fn copy_tree_links(
    src: &Path,
    dst: &Path,
    keep_existing: bool,
    created: &mut Vec<PathBuf>,
    roots: &[PathBuf],
    installed_links: &mut Vec<InstalledLink>,
) -> Result<()> {
    let dst = &std::path::absolute(dst).unwrap_or_else(|_| dst.to_path_buf());
    let mut walk = WalkDir::new(src).sort_by_file_name().into_iter();
    while let Some(entry) = walk.next() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        // `dst.join("")` ends in a slash, which `is_symlink` would follow
        let mut dest = if rel.as_os_str().is_empty() {
            dst.to_path_buf()
        } else {
            dst.join(rel)
        };
        let ft = entry.file_type();
        if ft.is_dir() {
            // `create_dir_all` accepts a link to a directory and everything
            // below would land where it points. The link can be one this tree
            // ships: it lands as `<name>.default` when `<name>` is taken. A
            // directory is visited before its contents and is never replaced
            // by a link, so every directory written to below `dst` is real.
            // `dst` itself (`<prefix>/etc`) is checked too: a link there would
            // take the whole tree outside the prefix.
            if dest.is_symlink() {
                warn!(
                    "brew: not installing {}: it is a symlink, and files are not copied through one",
                    dest.display()
                );
                walk.skip_current_dir();
                continue;
            }
            fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if ft.is_symlink() {
            let link = fs::read_link(entry.path())?;
            if !super::pour::link_stays_in(dest.parent().unwrap(), &link, None, roots) {
                warn!(
                    "brew: not installing {}: it is a symlink to {}, which points outside the \
                     Homebrew prefix",
                    dest.display(),
                    link.display(),
                );
                continue;
            }
            let mut existed = false;
            if let Ok(meta) = dest.symlink_metadata() {
                if meta.is_symlink() && fs::read_link(&dest).ok().as_deref() == Some(&link) {
                    continue;
                }
                if keep_existing {
                    // a differing link is the user's: the bottle's lands beside it
                    let mut name = dest.file_name().unwrap().to_os_string();
                    name.push(".default");
                    dest.set_file_name(name);
                }
                if let Ok(meta) = dest.symlink_metadata() {
                    if meta.is_dir() {
                        warn!(
                            "brew: not installing {}: a directory is in the way",
                            dest.display()
                        );
                        continue;
                    }
                    existed = true;
                }
            }
            // what the link replaces is kept aside until the link has passed
            // its check once the whole tree is in place
            let replaced = if existed {
                Some(Replaced::set_aside(&dest)?)
            } else {
                None
            };
            if let Err(err) = mise_util::file::make_symlink(&link, &dest) {
                if let Some(replaced) = replaced {
                    replaced.put_back(&dest);
                }
                return Err(err);
            }
            installed_links.push(InstalledLink {
                dest: dest.clone(),
                link,
                replaced,
            });
            // only what did not exist before is ours to remove on rollback
            if !existed {
                created.push(dest);
            }
            continue;
        }
        if keep_existing && let Ok(meta) = dest.symlink_metadata() {
            if !meta.is_file() {
                warn!(
                    "brew: not installing {}: something that is not a file is in the way",
                    dest.display()
                );
                continue;
            }
            if fs::read(&dest)? == fs::read(entry.path())? {
                continue;
            }
            let mut name = dest.file_name().unwrap().to_os_string();
            name.push(".default");
            dest.set_file_name(name);
            // mise only ever writes a file here, so a link is the user's,
            // and replacing it could not be undone
            if dest.is_symlink() {
                warn!(
                    "brew: not installing {}: it is a symlink, and it is left as it is",
                    dest.display()
                );
                continue;
            }
        }
        let existed = dest.symlink_metadata().is_ok();
        if dest.is_symlink() {
            // `fs::copy` writes to wherever the link points
            fs::remove_file(&dest)?;
        }
        fs::copy(entry.path(), &dest)?;
        if !existed {
            created.push(dest);
        }
    }
    Ok(())
}

/// A link [`copy_tree_links`] installed at `dest`, pointing to `link`.
struct InstalledLink {
    dest: PathBuf,
    link: PathBuf,
    /// the file or link that was at `dest` before
    replaced: Option<Replaced>,
}

/// A file or link moved out of the way of a link from the bottle, into a
/// directory of its own beside it. It is deleted with that directory when
/// this is dropped, once the new link is known to stay; until then it can be
/// put back.
struct Replaced {
    dir: tempfile::TempDir,
    path: PathBuf,
}

impl Replaced {
    fn set_aside(dest: &Path) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix(".mise-replaced-")
            .tempdir_in(dest.parent().unwrap())?;
        let path = dir.path().join(dest.file_name().unwrap());
        fs::rename(dest, &path)?;
        Ok(Self { dir, path })
    }

    /// Move the old entry back to `dest`, where the new link has been
    /// removed. If that fails, it is kept where it is and the warning says
    /// where that is.
    fn put_back(self, dest: &Path) {
        if let Err(err) = fs::rename(&self.path, dest) {
            let kept = self.dir.keep().join(dest.file_name().unwrap());
            warn!(
                "brew: could not put {} back ({err}); it is in {}",
                dest.display(),
                kept.display()
            );
        }
    }
}

/// Remove each link in `links` and put back what it replaced, after a later
/// step failed.
fn undo_links(links: Vec<InstalledLink>) {
    for installed in links {
        if let Some(replaced) = installed.replaced
            && fs::remove_file(&installed.dest).is_ok()
        {
            replaced.put_back(&installed.dest);
        }
    }
}

/// Check each of `links` from where it now is, and remove the ones that
/// point outside every one of `roots`, putting back what each replaced.
/// Removing one can change where another leads, so this repeats until a pass
/// removes nothing.
fn recheck_links(
    mut links: Vec<InstalledLink>,
    created: &mut Vec<PathBuf>,
    roots: &[PathBuf],
) -> Result<()> {
    loop {
        let (stay, leave): (Vec<_>, Vec<_>) = links.into_iter().partition(|installed| {
            super::pour::link_stays_in(
                installed.dest.parent().unwrap(),
                &installed.link,
                None,
                roots,
            )
        });
        if leave.is_empty() {
            return Ok(());
        }
        let mut failed = None;
        for installed in leave {
            warn!(
                "brew: removing {}: once the rest was copied, its link to {} points outside \
                 the Homebrew prefix",
                installed.dest.display(),
                installed.link.display(),
            );
            if let Err(err) = fs::remove_file(&installed.dest) {
                failed.get_or_insert(err);
                // the link is still there, so what it replaced stays aside
                if let Some(replaced) = installed.replaced {
                    let _ = replaced.dir.keep();
                }
                continue;
            }
            created.retain(|path| *path != installed.dest);
            if let Some(replaced) = installed.replaced {
                replaced.put_back(&installed.dest);
            }
        }
        if let Some(err) = failed {
            undo_links(stay);
            return Err(err.into());
        }
        links = stay;
    }
}

type GuardCache = HashMap<String, bool>;

pub(super) struct Context<'a> {
    pub name: &'a str,
    /// the formula's `versions.stable`, for `{{version.major}}` templates
    pub stable: Option<&'a str>,
    pub keg: &'a Path,
}

/// Run the steps in order. Like brew, a failing step is a warning, not an
/// install failure, and ends the remaining steps; an unsupported step is
/// reported and skipped.
pub(super) fn run_steps(steps: &[Value], ctx: &Context) {
    // brew decides each guard once per formula, so steps that share a guard id
    // keep one decision even after an earlier step creates or removes the path
    let mut guards = HashMap::new();
    for step in steps {
        let kind = step.get("type").and_then(Value::as_str).unwrap_or("?");
        match ctx.apply(step, kind, &mut guards) {
            Ok(()) => {}
            Err(err) => {
                if let Some(un) = err.downcast_ref::<Unsupported>() {
                    warn!(
                        "brew:{}: skipped post-install step `{kind}` ({un}); run `brew postinstall {}` if the formula needs it",
                        ctx.name, ctx.name
                    );
                } else {
                    warn!(
                        "brew:{}: post-install step `{kind}` failed: {err:#}; later steps were not run",
                        ctx.name
                    );
                    return;
                }
            }
        }
    }
}

impl Context<'_> {
    fn apply(&self, step: &Value, kind: &str, guards: &mut GuardCache) -> Result<()> {
        if !self.guards_pass(step, guards)? {
            return Ok(());
        }
        match kind {
            "mkdir_p" => {
                fs::create_dir_all(self.path(step, "path")?)?;
            }
            "touch" => {
                let path = self.path(step, "path")?;
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)?;
            }
            "symlink" => self.symlink(step)?,
            "copy" => self.copy(step)?,
            "move" => self.move_(step)?,
            "remove" => self.remove(step)?,
            "set_permissions" => self.set_permissions(step)?,
            "write" => self.write(step)?,
            "run" => self.run(step)?,
            other => return unsupported(format!("`{other}` is not implemented")),
        }
        Ok(())
    }

    fn symlink(&self, step: &Value) -> Result<()> {
        refuse_glob(step)?;
        let source = self.path(step, "source")?;
        let target = self.path(step, "target")?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if target.symlink_metadata().is_ok() {
            if !flag(step, "force") {
                return Ok(());
            }
            if target.is_dir() && !target.is_symlink() {
                bail!("{} is a directory", target.display());
            }
            fs::remove_file(&target)?;
        }
        mise_util::file::make_symlink(&source, &target)?;
        Ok(())
    }

    fn copy(&self, step: &Value) -> Result<()> {
        refuse_glob(step)?;
        let source = self.path(step, "source")?;
        let target = self.path(step, "target")?;
        let destination =
            resolve_copy_destination(&copy_destination(&source, &target)?, &prefix_roots())?;
        // Homebrew serializes `overwrite: false` only; an absent key (the DSL
        // default) allows overwriting, unlike `move`/`write` where `flag` applies.
        let protected = step.get("overwrite").and_then(Value::as_bool) == Some(false);
        if protected && destination.exists() {
            bail!("{} already exists", destination.display());
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        if source.is_dir() {
            if !flag(step, "recursive") {
                bail!("{} is a directory", source.display());
            }
            let roots = super::pour::link_roots();
            copy_tree(&source, &destination, false, &mut Vec::new(), &roots)
        } else {
            fs::copy(&source, &destination)?;
            Ok(())
        }
    }

    fn move_(&self, step: &Value) -> Result<()> {
        refuse_glob(step)?;
        let source = self.path(step, "source")?;
        let target = self.path(step, "target")?;
        if target.symlink_metadata().is_ok() {
            if !flag(step, "overwrite") {
                return Ok(());
            }
            mise_util::file::remove_all(&target)?;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        mise_util::file::move_file(&source, &target)?;
        Ok(())
    }

    fn remove(&self, step: &Value) -> Result<()> {
        if step.get("symlink_target_contains").is_some() {
            return unsupported("`symlink_target_contains` is not implemented");
        }
        let recursive = flag(step, "recursive");
        for path in self.paths(step)? {
            match path.symlink_metadata() {
                Err(_) => {}
                Ok(meta) if meta.is_dir() && recursive => fs::remove_dir_all(&path)?,
                Ok(meta) if meta.is_dir() => fs::remove_dir(&path)?,
                Ok(_) => fs::remove_file(&path)?,
            }
        }
        Ok(())
    }

    fn write(&self, step: &Value) -> Result<()> {
        let path = self.path(step, "path")?;
        if path.symlink_metadata().is_ok() && !flag(step, "overwrite") {
            return Ok(());
        }
        let content = step
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("missing `content`"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, self.expand(content)?)?;
        Ok(())
    }

    #[cfg(unix)]
    fn set_permissions(&self, step: &Value) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let spec = step
            .get("permissions")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("missing `permissions`"))?;
        let recursive = !flag(step, "non_recursive");
        for path in self.paths(step)? {
            let entries: Vec<PathBuf> = if recursive && path.is_dir() {
                WalkDir::new(&path)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter(|e| !e.path_is_symlink())
                    .map(|e| e.into_path())
                    .collect()
            } else {
                vec![path]
            };
            for entry in entries {
                let Ok(meta) = fs::metadata(&entry) else {
                    continue;
                };
                let mode = apply_mode(spec, meta.permissions().mode(), meta.is_dir())?;
                fs::set_permissions(&entry, fs::Permissions::from_mode(mode))?;
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn set_permissions(&self, _step: &Value) -> Result<()> {
        unsupported("file modes are not supported on this platform")
    }

    fn run(&self, step: &Value) -> Result<()> {
        let command = self.path(step, "command")?;
        let mut cmd = std::process::Command::new(&command);
        for arg in step
            .get("args")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let arg = arg.as_str().ok_or_else(|| eyre!("non-string argument"))?;
            cmd.arg(self.expand(arg)?);
        }
        let prefix = prefix::prefix();
        let path = std::env::join_paths(
            [
                self.keg.join("bin"),
                prefix.join("bin"),
                prefix.join("sbin"),
            ]
            .into_iter()
            .chain(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            )),
        )?;
        cmd.env("PATH", path)
            .env("HOMEBREW_PREFIX", &prefix)
            .env("HOMEBREW_CELLAR", prefix::cellar());
        if let Some(env) = step.get("env").and_then(Value::as_object) {
            for (key, value) in env {
                let value = value
                    .as_str()
                    .ok_or_else(|| eyre!("non-string env value"))?;
                cmd.env(key, self.expand(value)?);
            }
        }
        match step.get("chdir") {
            Some(dir) => cmd.current_dir(self.spec(dir)?),
            None => cmd.current_dir(self.keg),
        };
        if step.get("stdin_path").is_some() {
            cmd.stdin(fs::File::open(self.path(step, "stdin_path")?)?);
        } else {
            cmd.stdin(std::process::Stdio::null());
        }
        if step.get("stdout_path").is_some() {
            let out = self.path(step, "stdout_path")?;
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            cmd.stdout(fs::File::create(out)?);
        }
        let output = cmd
            .stderr(std::process::Stdio::piped())
            .output()
            .wrap_err_with(|| format!("failed to run {}", command.display()))?;
        if !output.status.success() {
            let mut msg = format!("{} exited with {}", command.display(), output.status);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                msg.push_str(&format!(": {}", stderr.trim()));
            }
            bail!(msg);
        }
        Ok(())
    }

    fn guards_pass(&self, step: &Value, cache: &mut GuardCache) -> Result<bool> {
        for guard in step
            .get("guards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = guard.get("id").and_then(Value::as_str);
            if let Some(&cached) = id.and_then(|id| cache.get(id)) {
                if !cached {
                    return Ok(false);
                }
                continue;
            }
            let condition = guard.get("condition").and_then(Value::as_str);
            let pass = match condition {
                Some("on") => match guard.get("value").and_then(Value::as_str) {
                    Some("macos") => cfg!(target_os = "macos"),
                    Some("linux") => cfg!(target_os = "linux"),
                    other => return unsupported(format!("guard `on` {other:?}")),
                },
                Some("if_exists") => self.spec(guard)?.exists(),
                Some("unless_exists") => !self.spec(guard)?.exists(),
                other => return unsupported(format!("guard {other:?}")),
            };
            if let Some(id) = id {
                cache.insert(id.to_string(), pass);
            }
            if !pass {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// the path object under `key`
    fn path(&self, step: &Value, key: &str) -> Result<PathBuf> {
        self.spec(step.get(key).ok_or_else(|| eyre!("missing `{key}`"))?)
    }

    /// the `paths` array, with glob patterns expanded
    fn paths(&self, step: &Value) -> Result<Vec<PathBuf>> {
        let list = step
            .get("paths")
            .and_then(Value::as_array)
            .ok_or_else(|| eyre!("missing `paths`"))?;
        let mut out = Vec::new();
        for item in list {
            let path = self.spec(item)?;
            let text = path.to_string_lossy();
            if text.contains(['*', '?', '[']) {
                for m in glob::glob(&text)? {
                    out.push(m?);
                }
            } else {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// resolve a `{base?, path}` object to an absolute path inside brew's tree
    fn spec(&self, spec: &Value) -> Result<PathBuf> {
        let path = self.expand(
            spec.get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| eyre!("missing `path`"))?,
        )?;
        let path = Path::new(&path);
        if path.components().any(|c| c == Component::ParentDir) {
            bail!("path {} contains `..`", path.display());
        }
        match spec.get("base").and_then(Value::as_str) {
            Some(base) => {
                if path.is_absolute() {
                    bail!("path {} is absolute but has a base", path.display());
                }
                Ok(self.base(base)?.join(path))
            }
            None if path.is_absolute() => Ok(path.to_path_buf()),
            None => bail!("path {} is relative but has no base", path.display()),
        }
    }

    fn base(&self, base: &str) -> Result<PathBuf> {
        let prefix = prefix::prefix();
        Ok(match base {
            "homebrew_prefix" => prefix,
            "etc" | "var" => prefix.join(base),
            "prefix" => self.keg.to_path_buf(),
            "bin" | "sbin" | "lib" | "libexec" | "share" => self.keg.join(base),
            "frameworks" => self.keg.join("Frameworks"),
            other => return unsupported(format!("base `{other}`")),
        })
    }

    fn expand(&self, input: &str) -> Result<String> {
        let mut out = String::new();
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after.find("}}").ok_or_else(|| eyre!("unclosed `{{{{`"))?;
            out.push_str(&self.variable(after[..end].trim())?);
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        Ok(out)
    }

    fn variable(&self, key: &str) -> Result<String> {
        let prefix = prefix::prefix();
        let join = |p: PathBuf| p.to_string_lossy().into_owned();
        Ok(match key {
            "HOMEBREW_PREFIX" => join(prefix),
            "HOMEBREW_CELLAR" => join(prefix::cellar()),
            "etc" | "var" => join(prefix.join(key)),
            "pkgetc" => join(prefix.join("etc").join(self.name)),
            "opt_prefix" => join(prefix.join("opt").join(self.name)),
            "prefix" => join(self.keg.to_path_buf()),
            "bin" | "sbin" | "lib" | "libexec" => join(self.keg.join(key)),
            "pkgshare" => join(self.keg.join("share").join(self.name)),
            "formula_name" => self.name.to_string(),
            "version.major" | "version.major_minor" => {
                // brew's Version#major/#major_minor split on dots and take
                // the leading components; no ordering is implied
                let stable = self.stable.ok_or_else(|| eyre!("formula has no version"))?;
                let take = if key == "version.major" { 1 } else { 2 };
                stable.split('.').take(take).collect::<Vec<_>>().join(".")
            }
            other => return unsupported(format!("template `{{{{{other}}}}}`")),
        })
    }
}

fn flag(step: &Value, key: &str) -> bool {
    step.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn refuse_glob(step: &Value) -> Result<()> {
    if flag(step, "source_glob") {
        return unsupported("`source_glob` is not implemented");
    }
    Ok(())
}

/// The Homebrew prefix and its Cellar, as spelled and as resolved: the
/// directories a step's paths start from.
fn prefix_roots() -> Vec<PathBuf> {
    [prefix::prefix(), prefix::cellar()]
        .into_iter()
        .chain(super::pour::link_roots())
        .collect()
}

/// Where a `copy` step writes, with the symlinks on the way resolved.
/// `copy_destination` follows a target that links to a directory, and
/// `fs::copy` and `copy_tree` would follow any other link, so a parent such
/// as `var/redirect -> /outside` or a destination file such as
/// `var/foo -> /outside/file` would write outside the prefix. Each link below
/// the deepest of `roots` the destination lies under, and the destination
/// itself, is resolved, and the copy goes on only where it lands below one of
/// `roots` again: `opt/<name>` leading to the formula's keg is followed, while
/// a link out of the prefix is an error. A link to something not there yet
/// (a file in the keg the step itself creates) is resolved as far as the
/// filesystem goes and taken as written from there, like the kernel does when
/// it creates the file. A link that goes on past an existing file is an
/// error, as it is for the kernel. The roots themselves may be links (a Cellar on
/// another disk), which `prefix::refuse_foreign_as_root` judges; a
/// destination below no root has only its own link checked.
fn resolve_copy_destination(destination: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let follow = |link: &Path| -> Result<PathBuf> {
        let real = super::pour::resolve_symlinks(link, None).ok_or_else(|| {
            eyre!(
                "not copying to {}: {} is a symlink mise cannot follow",
                destination.display(),
                link.display(),
            )
        })?;
        if !roots.iter().any(|root| real.starts_with(root)) {
            bail!(
                "not copying to {}: {} is a symlink to {}, outside the Homebrew prefix",
                destination.display(),
                link.display(),
                real.display(),
            );
        }
        Ok(real)
    };
    let (Some(parent), Some(name)) = (destination.parent(), destination.file_name()) else {
        return Ok(destination.to_path_buf());
    };
    let root = roots
        .iter()
        .filter(|root| parent.starts_with(root))
        .max_by_key(|root| root.components().count());
    let mut path = match root {
        Some(root) => {
            let mut path = root.clone();
            for part in parent.strip_prefix(root)?.components() {
                path.push(part);
                if path.is_symlink() {
                    path = follow(&path)?;
                }
            }
            path
        }
        None => parent.to_path_buf(),
    };
    path.push(name);
    if path.is_symlink() {
        path = follow(&path)?;
    }
    Ok(path)
}

/// Homebrew's copy steps place a source under an existing directory target.
/// Resolve that path before checking `overwrite`, since it is the path Homebrew
/// protects (rather than the directory containing it).
fn copy_destination(source: &Path, target: &Path) -> Result<PathBuf> {
    if target.is_dir() {
        let name = source
            .file_name()
            .ok_or_else(|| eyre!("source {} has no file name", source.display()))?;
        Ok(target.join(name))
    } else {
        Ok(target.to_path_buf())
    }
}

/// Apply an octal (`0700`) or symbolic (`u+w`, `go-rwx`, `a=rx`) mode spec.
#[cfg(unix)]
fn apply_mode(spec: &str, current: u32, is_dir: bool) -> Result<u32> {
    if !spec.is_empty() && spec.chars().all(|c| c.is_digit(8)) {
        return Ok(u32::from_str_radix(spec, 8)?);
    }
    let mut mode = current & 0o7777;
    for clause in spec.split(',') {
        let op_at = clause
            .find(['+', '-', '='])
            .ok_or_else(|| eyre!("unrecognized permissions `{spec}`"))?;
        let (who, rest) = clause.split_at(op_at);
        let op = rest.as_bytes()[0];
        let mut bits = 0;
        let mut perm = 0;
        for c in rest[1..].chars() {
            perm |= match c {
                'r' => 0o4,
                'w' => 0o2,
                'x' => 0o1,
                'X' if is_dir || current & 0o111 != 0 => 0o1,
                'X' => 0,
                _ => bail!("unrecognized permissions `{spec}`"),
            };
        }
        let who = if who.is_empty() { "a" } else { who };
        let mut mask = 0;
        for c in who.chars() {
            match c {
                'u' => (bits |= perm << 6, mask |= 0o700),
                'g' => (bits |= perm << 3, mask |= 0o070),
                'o' => (bits |= perm, mask |= 0o007),
                'a' => (bits |= perm * 0o111, mask |= 0o777),
                _ => bail!("unrecognized permissions `{spec}`"),
            };
        }
        match op {
            b'+' => mode |= bits,
            b'-' => mode &= !bits,
            _ => mode = (mode & !mask) | bits,
        }
    }
    Ok(mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(keg: &'a Path) -> Context<'a> {
        Context {
            name: "demo@4",
            stable: Some("4.0.3"),
            keg,
        }
    }

    #[test]
    fn expands_templates_and_resolves_bases() {
        let keg = Path::new("/p/Cellar/demo@4/4.0.3");
        let c = ctx(keg);
        assert_eq!(
            c.expand("{{formula_name}}-{{version.major}}").unwrap(),
            "demo@4-4"
        );
        assert_eq!(c.expand("{{version.major_minor}}").unwrap(), "4.0");
        let spec = json!({"base": "libexec", "path": "post-install"});
        assert_eq!(c.spec(&spec).unwrap(), keg.join("libexec/post-install"));
        assert!(c.spec(&json!({"base": "etc", "path": "../x"})).is_err());
        assert!(c.spec(&json!({"path": "relative"})).is_err());
        let err = c.expand("{{bash_completion}}").unwrap_err();
        assert!(err.downcast_ref::<Unsupported>().is_some());
    }

    #[test]
    fn steps_sharing_a_guard_id_keep_one_decision() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let marker = tmp.path().join("marker");
        let guard =
            json!([{"path": marker.to_str().unwrap(), "condition": "unless_exists", "id": "1"}]);
        let steps = vec![
            json!({"type": "touch", "path": {"path": marker.to_str().unwrap()}, "guards": guard}),
            json!({"type": "touch", "path": {"path": tmp.path().join("second").to_str().unwrap()}, "guards": guard}),
        ];
        run_steps(&steps, &ctx(tmp.path()));
        assert!(marker.exists());
        assert!(tmp.path().join("second").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn copy_tree_replaces_or_places_differing_symlinks() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;
        symlink("new", src.join("l"))?;
        symlink("old", dst.join("l"))?;
        copy_tree(
            &src,
            &dst,
            true,
            &mut Vec::new(),
            &[tmp.path().canonicalize()?],
        )?;
        assert_eq!(fs::read_link(dst.join("l"))?, Path::new("old"));
        assert_eq!(fs::read_link(dst.join("l.default"))?, Path::new("new"));
        copy_tree(
            &src,
            &dst,
            false,
            &mut Vec::new(),
            &[tmp.path().canonicalize()?],
        )?;
        assert_eq!(fs::read_link(dst.join("l"))?, Path::new("new"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn copy_tree_does_not_write_through_destination_symlinks() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let src = root.join("src");
        let dst = prefix.join("etc");
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        fs::write(outside.join("kept"), "kept")?;

        // a link the tree ships lands as `d.default`, the name of a directory
        // it ships as well
        fs::create_dir_all(src.join("d.default"))?;
        fs::create_dir_all(dst.join("d"))?;
        symlink(&outside, src.join("d"))?;
        fs::write(src.join("d.default/f"), "f")?;
        // a link left where a `.default` file goes
        fs::write(src.join("a"), "new")?;
        fs::write(dst.join("a"), "edited")?;
        symlink(outside.join("kept"), dst.join("a.default"))?;

        let mut created = Vec::new();
        copy_tree(
            &src,
            &dst,
            true,
            &mut created,
            std::slice::from_ref(&prefix),
        )?;
        assert!(!outside.join("f").exists());
        assert_eq!(fs::read_to_string(outside.join("kept"))?, "kept");
        // the user's link is neither written through nor replaced
        assert_eq!(fs::read_link(dst.join("a.default"))?, outside.join("kept"));
        assert!(!created.contains(&dst.join("a.default")));

        // the same without `keep_existing`
        symlink(&outside, dst.join("sub"))?;
        symlink(outside.join("kept"), dst.join("b"))?;
        fs::create_dir_all(src.join("sub"))?;
        fs::write(src.join("sub/g"), "g")?;
        fs::write(src.join("b"), "b")?;
        copy_tree(&src, &dst, false, &mut Vec::new(), &[prefix])?;
        assert!(!outside.join("f").exists());
        assert!(!outside.join("g").exists());
        assert_eq!(fs::read_to_string(outside.join("kept"))?, "kept");
        assert_eq!(fs::read_to_string(dst.join("b"))?, "b");
        Ok(())
    }

    /// `<prefix>/etc` itself is a link out of the prefix
    #[cfg(unix)]
    #[test]
    fn copy_tree_does_not_write_through_a_destination_root_symlink() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let src = root.join("src");
        let dst = prefix.join("etc");
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        fs::create_dir_all(src.join("sub"))?;
        fs::create_dir_all(&prefix)?;
        fs::write(src.join("a"), "a")?;
        fs::write(src.join("sub/b"), "b")?;
        symlink(&outside, &dst)?;
        for keep_existing in [true, false] {
            let mut created = Vec::new();
            copy_tree(
                &src,
                &dst,
                keep_existing,
                &mut created,
                std::slice::from_ref(&prefix),
            )?;
            assert!(created.is_empty(), "{created:?}");
            assert_eq!(fs::read_dir(&outside)?.count(), 0);
        }
        Ok(())
    }

    /// `etc/a -> b/x` passes while `etc/b` leads to `inside`, but the tree
    /// then replaces `etc/b` with a link to `inside2`, where `x` leads out of
    /// the prefix: `etc/a` is checked again and removed
    #[cfg(unix)]
    #[test]
    fn copy_tree_rechecks_links_once_the_tree_is_in_place() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        fs::create_dir_all(prefix.join("inside"))?;
        fs::create_dir_all(prefix.join("inside2"))?;
        fs::create_dir_all(prefix.join("etc"))?;
        symlink(&outside, prefix.join("inside2/x"))?;
        symlink("../inside", prefix.join("etc/b"))?;
        let src = root.join("src");
        fs::create_dir_all(&src)?;
        symlink("b/x", src.join("a"))?;
        symlink("../inside2", src.join("b"))?;
        let mut created = Vec::new();
        copy_tree(
            &src,
            &prefix.join("etc"),
            false,
            &mut created,
            std::slice::from_ref(&prefix),
        )?;
        assert!(prefix.join("etc/a").symlink_metadata().is_err());
        assert!(!created.contains(&prefix.join("etc/a")), "{created:?}");
        assert_eq!(
            fs::read_link(prefix.join("etc/b"))?,
            PathBuf::from("../inside2")
        );

        // a file the removed link replaced is put back, and the link that
        // stays does not keep what it replaced
        fs::write(prefix.join("etc/a"), "mine")?;
        fs::remove_file(prefix.join("etc/b"))?;
        symlink("../inside", prefix.join("etc/b"))?;
        let mut created = Vec::new();
        copy_tree(
            &src,
            &prefix.join("etc"),
            false,
            &mut created,
            std::slice::from_ref(&prefix),
        )?;
        assert_eq!(fs::read_to_string(prefix.join("etc/a"))?, "mine");
        assert!(created.is_empty(), "{created:?}");
        assert_eq!(
            fs::read_link(prefix.join("etc/b"))?,
            PathBuf::from("../inside2")
        );
        let mut names = fs::read_dir(prefix.join("etc"))?
            .map(|entry| Ok(entry?.file_name()))
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        assert_eq!(names, ["a", "b"]);
        Ok(())
    }

    /// `etc/a -> ../var/b/x` passes while `var/b` is missing, but the bottle's
    /// `var` then adds `var/b` as a link to `inside`, where `x` leads out of the
    /// prefix: the links of both trees are checked again together
    #[cfg(unix)]
    #[test]
    fn bottle_config_rechecks_etc_links_after_var_is_copied() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        fs::create_dir_all(prefix.join("inside"))?;
        symlink(&outside, prefix.join("inside/x"))?;
        let bottle = root.join("bottle");
        fs::create_dir_all(bottle.join("etc"))?;
        fs::create_dir_all(bottle.join("var"))?;
        symlink("../var/b/x", bottle.join("etc/a"))?;
        symlink("../inside", bottle.join("var/b"))?;
        let roots = [prefix.clone()];
        let mut created = Vec::new();
        let mut links = Vec::new();
        for dir in ["etc", "var"] {
            copy_tree_links(
                &bottle.join(dir),
                &prefix.join(dir),
                true,
                &mut created,
                &roots,
                &mut links,
            )?;
        }
        // each tree on its own passes its own check
        assert!(prefix.join("etc/a").is_symlink());
        recheck_links(links, &mut created, &roots)?;
        assert!(prefix.join("etc/a").symlink_metadata().is_err());
        assert!(!created.contains(&prefix.join("etc/a")), "{created:?}");
        assert_eq!(
            fs::read_link(prefix.join("var/b"))?,
            PathBuf::from("../inside")
        );
        Ok(())
    }

    /// a relative destination (a relative `MISE_SYSTEM_BREW_PREFIX`) has its
    /// links checked from the working directory, not from `/`
    #[cfg(unix)]
    #[test]
    fn copy_tree_checks_links_of_a_relative_destination_from_the_working_directory() -> Result<()> {
        use std::os::unix::fs::symlink;
        let cwd = std::env::current_dir()?;
        let tmp = tempfile::tempdir_in(&cwd)?;
        let name = PathBuf::from(tmp.path().file_name().unwrap());
        let src = tmp.path().join("src");
        fs::create_dir_all(&src)?;
        fs::write(src.join("file"), "f")?;
        symlink("file", src.join("link"))?;
        let prefix = cwd.join(&name).join("prefix");
        fs::create_dir_all(&prefix)?;
        let roots = [prefix.canonicalize()?];
        let mut created = Vec::new();
        copy_tree(&src, &name.join("prefix/etc"), false, &mut created, &roots)?;
        let link = prefix.join("etc/link");
        assert_eq!(fs::read_link(&link)?, PathBuf::from("file"));
        assert!(created.contains(&link), "{created:?}");
        Ok(())
    }

    /// a link that stays in the keg can leave the prefix once it is copied
    /// into etc; links between prefix directories still install
    #[cfg(unix)]
    #[test]
    fn copy_tree_installs_only_links_that_stay_in_the_prefix() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let src = root.join("src");
        let dst = prefix.join("etc");
        fs::create_dir_all(src.join("openssl"))?;
        fs::create_dir_all(&dst)?;
        symlink("../ca-certificates/cert.pem", src.join("openssl/cert.pem"))?;
        symlink("../Cellar/foo/1.0/share/foo", src.join("foo"))?;
        symlink("../../outside", src.join("up"))?;
        symlink(root.join("outside"), src.join("abs"))?;
        let mut created = Vec::new();
        copy_tree(&src, &dst, true, &mut created, &[prefix])?;
        assert_eq!(
            fs::read_link(dst.join("openssl/cert.pem"))?,
            Path::new("../ca-certificates/cert.pem")
        );
        assert!(dst.join("foo").is_symlink());
        assert!(dst.join("up").symlink_metadata().is_err());
        assert!(dst.join("abs").symlink_metadata().is_err());
        assert_eq!(created.len(), 2);
        Ok(())
    }

    /// links are checked one at a time, so a `..` through a link the bottle
    /// installs later is refused: `a` is checked while `b` does not exist yet,
    /// and once `b` points at the prefix, `a` would point above it
    #[cfg(unix)]
    #[test]
    fn copy_tree_refuses_a_parent_step_through_a_link_not_yet_installed() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let src = root.join("src");
        let dst = prefix.join("etc");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;
        symlink("b/../..", src.join("a"))?;
        symlink("..", src.join("b"))?;
        let mut created = Vec::new();
        copy_tree(&src, &dst, true, &mut created, &[prefix])?;
        assert!(dst.join("a").symlink_metadata().is_err());
        assert!(dst.join("b").is_symlink());
        Ok(())
    }

    #[test]
    fn modes() {
        assert_eq!(apply_mode("0700", 0o644, false).unwrap(), 0o700);
        assert_eq!(apply_mode("u+w", 0o444, false).unwrap(), 0o644);
        assert_eq!(apply_mode("go-rwx", 0o755, false).unwrap(), 0o700);
        assert_eq!(apply_mode("a=rx", 0o600, false).unwrap(), 0o555);
        assert!(apply_mode("bogus", 0o600, false).is_err());
    }

    #[test]
    fn copy_tree_keeps_differing_files_as_default() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        fs::create_dir_all(src.join("sub"))?;
        fs::create_dir_all(&dst)?;
        fs::write(src.join("a.cnf"), "new")?;
        fs::write(src.join("same"), "x")?;
        fs::write(src.join("sub/b"), "b")?;
        fs::write(dst.join("a.cnf"), "edited")?;
        fs::write(dst.join("same"), "x")?;
        copy_tree(
            &src,
            &dst,
            true,
            &mut Vec::new(),
            &[tmp.path().canonicalize()?],
        )?;
        assert_eq!(fs::read_to_string(dst.join("a.cnf"))?, "edited");
        assert_eq!(fs::read_to_string(dst.join("a.cnf.default"))?, "new");
        assert!(!dst.join("same.default").exists());
        assert_eq!(fs::read_to_string(dst.join("sub/b"))?, "b");
        Ok(())
    }

    #[test]
    fn copy_without_overwrite_preserves_existing_destination() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let destination = tmp.path().join("destination");
        fs::write(&source, "new")?;
        fs::write(&destination, "old")?;

        let step = json!({
            "source": {"path": source},
            "target": {"path": destination},
            "overwrite": false,
        });
        assert!(ctx(tmp.path()).copy(&step).is_err());
        assert_eq!(fs::read_to_string(destination)?, "old");
        Ok(())
    }

    #[test]
    fn copy_without_overwrite_creates_absent_file_and_directory_destinations() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let file_source = tmp.path().join("file-source");
        let file_destination = tmp.path().join("file-destination");
        fs::write(&file_source, "file")?;
        let file_step = json!({
            "source": {"path": file_source},
            "target": {"path": file_destination},
            "overwrite": false,
        });
        ctx(tmp.path()).copy(&file_step)?;
        assert_eq!(fs::read_to_string(file_destination)?, "file");

        let directory_source = tmp.path().join("directory-source");
        let directory_target = tmp.path().join("directory-target");
        fs::create_dir_all(directory_source.join("nested"))?;
        fs::create_dir_all(&directory_target)?;
        fs::write(directory_source.join("nested/file"), "directory")?;
        let directory_step = json!({
            "source": {"path": directory_source},
            "target": {"path": directory_target},
            "recursive": true,
            "overwrite": false,
        });
        ctx(tmp.path()).copy(&directory_step)?;
        assert_eq!(
            fs::read_to_string(directory_target.join("directory-source/nested/file"))?,
            "directory"
        );
        Ok(())
    }

    /// a recursive copy onto a symlink copies into where it leads in the
    /// prefix and fails for anywhere else; it is never skipped
    #[cfg(unix)]
    #[test]
    fn recursive_copy_onto_a_symlink_follows_it_only_within_the_prefix() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let inside = prefix.join("Cellar/demo/etc/conf");
        fs::create_dir_all(&inside)?;
        symlink(&inside, prefix.join("conf"))?;
        let roots = [prefix.clone()];
        assert_eq!(
            resolve_copy_destination(&prefix.join("conf"), &roots)?,
            inside
        );
        assert_eq!(resolve_copy_destination(&inside, &roots)?, inside);
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        symlink(&outside, prefix.join("away"))?;
        let err = resolve_copy_destination(&prefix.join("away"), &roots).unwrap_err();
        assert!(
            err.to_string().contains("outside the Homebrew prefix"),
            "{err}"
        );
        symlink(root.join("missing"), prefix.join("dangling"))?;
        assert!(resolve_copy_destination(&prefix.join("dangling"), &roots).is_err());

        // the step itself: `<target>/conf` links out of the (default) prefix
        let source = root.join("src/conf");
        fs::create_dir_all(&source)?;
        fs::write(source.join("f"), "f")?;
        let target = root.join("target");
        fs::create_dir_all(&target)?;
        symlink(&outside, target.join("conf"))?;
        let step = json!({
            "source": {"path": source},
            "target": {"path": target},
            "recursive": true,
        });
        let err = ctx(&root).copy(&step).unwrap_err();
        assert!(
            err.to_string().contains("outside the Homebrew prefix"),
            "{err}"
        );
        assert_eq!(fs::read_dir(&outside)?.count(), 0);
        Ok(())
    }

    /// `var/redirect` links out of the prefix: a copy below it is refused,
    /// while real directories, a linked Cellar and `opt/demo` leading to the
    /// formula's keg are fine, and a copy below `opt/demo` lands in the keg
    #[cfg(unix)]
    #[test]
    fn copy_follows_links_in_the_prefix_only_where_they_stay_inside() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        let outside = root.join("outside");
        let disk = root.join("disk/cellar");
        fs::create_dir_all(prefix.join("var/real"))?;
        fs::create_dir_all(&outside)?;
        fs::create_dir_all(disk.join("demo/1.0"))?;
        symlink(&outside, prefix.join("var/redirect"))?;
        symlink(&disk, prefix.join("Cellar"))?;
        fs::create_dir_all(prefix.join("opt"))?;
        symlink("../Cellar/demo/1.0", prefix.join("opt/demo"))?;
        let roots = [prefix.clone(), prefix.join("Cellar"), disk.clone()];

        for destination in [
            prefix.join("var/redirect/assets"),
            prefix.join("var/redirect/deeper/assets"),
        ] {
            let err = resolve_copy_destination(&destination, &roots).unwrap_err();
            assert!(
                err.to_string().contains(&format!(
                    "{} is a symlink to {}, outside the Homebrew prefix",
                    prefix.join("var/redirect").display(),
                    outside.display(),
                )),
                "{err}"
            );
        }
        for (destination, resolved) in [
            (
                prefix.join("var/real/assets"),
                prefix.join("var/real/assets"),
            ),
            (prefix.join("var/new/assets"), prefix.join("var/new/assets")),
            (
                prefix.join("Cellar/demo/1.0/etc"),
                prefix.join("Cellar/demo/1.0/etc"),
            ),
            (prefix.join("opt/demo/etc/x"), disk.join("demo/1.0/etc/x")),
            (outside.join("assets"), outside.join("assets")),
        ] {
            let got = resolve_copy_destination(&destination, &roots)
                .wrap_err_with(|| destination.display().to_string())?;
            assert_eq!(got, resolved, "{}", destination.display());
        }
        Ok(())
    }

    /// `var/via -> ../etc/real/../victim` leads nowhere: the kernel refuses to
    /// go through the file `etc/real`, so the copy must not write `etc/victim`,
    /// which dropping `real/..` would give. Missing components still resolve.
    #[cfg(unix)]
    #[test]
    fn copy_does_not_follow_a_destination_link_through_a_file() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().canonicalize()?.join("prefix");
        fs::create_dir_all(prefix.join("var"))?;
        fs::create_dir_all(prefix.join("etc"))?;
        fs::write(prefix.join("etc/real"), "real")?;
        fs::write(prefix.join("etc/victim"), "keep")?;
        let roots = [prefix.clone()];
        for target in ["../etc/real/../victim", "../etc/real/victim"] {
            let via = prefix.join("var/via");
            let _ = fs::remove_file(&via);
            symlink(target, &via)?;
            let err = resolve_copy_destination(&via, &roots).unwrap_err();
            assert!(err.to_string().contains("cannot follow"), "{target}: {err}");
            assert!(!super::super::pour::link_stays_in(
                &prefix.join("var"),
                Path::new(target),
                None,
                &roots
            ));
        }
        // a missing directory is taken as written, and a file at the end is fine
        let via = prefix.join("var/via");
        fs::remove_file(&via)?;
        symlink("../etc/new/file", &via)?;
        assert_eq!(
            resolve_copy_destination(&via, &roots)?,
            prefix.join("etc/new/file")
        );
        fs::remove_file(&via)?;
        symlink("../etc/real", &via)?;
        assert_eq!(
            resolve_copy_destination(&via, &roots)?,
            prefix.join("etc/real")
        );
        assert_eq!(fs::read_to_string(prefix.join("etc/victim"))?, "keep");
        Ok(())
    }

    /// a file copied onto `var/foo -> /outside/file` would be written through
    /// the link; it is refused, and a link to a file in the prefix is followed
    #[cfg(unix)]
    #[test]
    fn copy_does_not_write_a_file_through_a_destination_link_out_of_the_prefix() -> Result<()> {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().canonicalize()?;
        let prefix = root.join("prefix");
        fs::create_dir_all(prefix.join("var"))?;
        fs::create_dir_all(prefix.join("etc"))?;
        let outside = root.join("outside/file");
        fs::create_dir_all(outside.parent().unwrap())?;
        fs::write(&outside, "keep")?;
        symlink(&outside, prefix.join("var/foo"))?;
        fs::write(prefix.join("etc/real"), "old")?;
        symlink(prefix.join("etc/real"), prefix.join("var/inside"))?;
        let roots = [prefix.clone()];

        let err = resolve_copy_destination(&prefix.join("var/foo"), &roots).unwrap_err();
        assert!(
            err.to_string().contains("outside the Homebrew prefix"),
            "{err}"
        );
        assert_eq!(
            resolve_copy_destination(&prefix.join("var/inside"), &roots)?,
            prefix.join("etc/real")
        );
        // a link to a file the step is about to create in the keg, and one to
        // a missing file outside the prefix
        symlink("../Cellar/demo/1.0/later", prefix.join("var/later"))?;
        assert_eq!(
            resolve_copy_destination(&prefix.join("var/later"), &roots)?,
            prefix.join("Cellar/demo/1.0/later")
        );
        symlink(root.join("outside/missing"), prefix.join("var/gone"))?;
        let err = resolve_copy_destination(&prefix.join("var/gone"), &roots).unwrap_err();
        assert!(
            err.to_string().contains("outside the Homebrew prefix"),
            "{err}"
        );

        // the step itself, with a destination link outside the (default) prefix
        let source = root.join("src/file");
        fs::create_dir_all(source.parent().unwrap())?;
        fs::write(&source, "new")?;
        let target = root.join("target");
        fs::create_dir_all(&target)?;
        symlink(&outside, target.join("file"))?;
        let step = json!({
            "source": {"path": source},
            "target": {"path": target.join("file")},
        });
        let err = ctx(&root).copy(&step).unwrap_err();
        assert!(
            err.to_string().contains("outside the Homebrew prefix"),
            "{err}"
        );
        assert_eq!(fs::read_to_string(&outside)?, "keep");
        Ok(())
    }

    #[test]
    fn copy_defaults_to_overwriting_existing_destination() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let source = tmp.path().join("source");
        let destination = tmp.path().join("destination");
        fs::write(&source, "new")?;
        fs::write(&destination, "old")?;

        let step = json!({
            "source": {"path": source},
            "target": {"path": destination},
        });
        ctx(tmp.path()).copy(&step)?;
        assert_eq!(fs::read_to_string(destination)?, "new");
        Ok(())
    }

    #[test]
    fn copy_without_overwrite_preserves_existing_child_of_directory_target() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let target = tmp.path().join("target");
        fs::create_dir_all(&target)?;

        let file_source = tmp.path().join("file-source");
        fs::write(&file_source, "new")?;
        fs::write(target.join("file-source"), "old")?;
        let file_step = json!({
            "source": {"path": file_source},
            "target": {"path": target},
            "overwrite": false,
        });
        assert!(ctx(tmp.path()).copy(&file_step).is_err());
        assert_eq!(fs::read_to_string(target.join("file-source"))?, "old");

        let dir_source = tmp.path().join("dir-source");
        fs::create_dir_all(&dir_source)?;
        fs::write(dir_source.join("a"), "new")?;
        fs::create_dir_all(target.join("dir-source"))?;
        fs::write(target.join("dir-source/a"), "old")?;
        let dir_step = json!({
            "source": {"path": dir_source},
            "target": {"path": target},
            "recursive": true,
            "overwrite": false,
        });
        assert!(ctx(tmp.path()).copy(&dir_step).is_err());
        assert_eq!(fs::read_to_string(target.join("dir-source/a"))?, "old");
        Ok(())
    }
}
