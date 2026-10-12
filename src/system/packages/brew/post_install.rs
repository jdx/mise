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
use crate::result::Result;

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
    let copied = ["etc", "var"].into_iter().try_for_each(|dir| {
        let src = bottle.join(dir);
        if src.is_dir() {
            copy_tree(&src, &prefix.join(dir), true, &mut created)
                .wrap_err_with(|| format!("failed to install the bottle's {dir} files"))?;
        }
        Ok(())
    });
    let result = copied.and_then(|()| crate::file::remove_all(&bottle));
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
fn copy_tree(
    src: &Path,
    dst: &Path,
    keep_existing: bool,
    created: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in WalkDir::new(src).sort_by_file_name() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        let mut dest = dst.join(rel);
        let ft = entry.file_type();
        if ft.is_dir() {
            fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        if ft.is_symlink() {
            let link = fs::read_link(entry.path())?;
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
                    fs::remove_file(&dest)?;
                }
            }
            crate::file::make_symlink(&link, &dest)?;
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
        }
        let existed = dest.symlink_metadata().is_ok();
        fs::copy(entry.path(), &dest)?;
        if !existed {
            created.push(dest);
        }
    }
    Ok(())
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
        crate::file::make_symlink(&source, &target)?;
        Ok(())
    }

    fn copy(&self, step: &Value) -> Result<()> {
        refuse_glob(step)?;
        let source = self.path(step, "source")?;
        let target = self.path(step, "target")?;
        let destination = copy_destination(&source, &target)?;
        // Homebrew serializes `overwrite: false` only; an absent key (the DSL
        // default) allows overwriting, unlike `move`/`write` where `flag` applies.
        let protected = step.get("overwrite").and_then(Value::as_bool) == Some(false);
        // `exists()` follows symlinks, so a dangling link is not protected,
        // matching Homebrew's `destination.exist?`.
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
            copy_tree(&source, &destination, false, &mut Vec::new())
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
            crate::file::remove_all(&target)?;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        crate::file::move_file(&source, &target)?;
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
        copy_tree(&src, &dst, true, &mut Vec::new())?;
        assert_eq!(fs::read_link(dst.join("l"))?, Path::new("old"));
        assert_eq!(fs::read_link(dst.join("l.default"))?, Path::new("new"));
        copy_tree(&src, &dst, false, &mut Vec::new())?;
        assert_eq!(fs::read_link(dst.join("l"))?, Path::new("new"));
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
        copy_tree(&src, &dst, true, &mut Vec::new())?;
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
