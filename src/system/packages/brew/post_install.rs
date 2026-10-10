//! What Homebrew does after pouring a bottle: copy the `etc`/`var` files the
//! bottle ships, then run the formula's declarative `post_install_steps` from
//! the Homebrew API. Only the generic step types are implemented; the
//! tool-specific ones (`compile_gsettings_schemas`, `init_data_dir`, ...) are
//! reported and skipped, since mise never runs `brew`.

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
pub(super) fn install_bottle_config(keg: &Path) -> Result<()> {
    let bottle = keg.join(".bottle");
    if !bottle.is_dir() {
        return Ok(());
    }
    let prefix = prefix::prefix();
    for dir in ["etc", "var"] {
        let src = bottle.join(dir);
        if src.is_dir() {
            copy_tree(&src, &prefix.join(dir), true)
                .wrap_err_with(|| format!("failed to install the bottle's {dir} files"))?;
        }
    }
    crate::file::remove_all(&bottle)
}

/// Copy `src` into `dst`. With `keep_existing`, an existing different file is
/// preserved and the new one is written to `<file>.default`.
fn copy_tree(src: &Path, dst: &Path, keep_existing: bool) -> Result<()> {
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
            if dest.symlink_metadata().is_err() {
                crate::file::make_symlink(&fs::read_link(entry.path())?, &dest)?;
            }
            continue;
        }
        if keep_existing && let Ok(meta) = dest.symlink_metadata() {
            if !meta.is_file() {
                continue;
            }
            if fs::read(&dest)? == fs::read(entry.path())? {
                continue;
            }
            let mut name = dest.file_name().unwrap().to_os_string();
            name.push(".default");
            dest.set_file_name(name);
        }
        fs::copy(entry.path(), &dest)?;
    }
    Ok(())
}

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
    for step in steps {
        let kind = step.get("type").and_then(Value::as_str).unwrap_or("?");
        match ctx.apply(step, kind) {
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
    fn apply(&self, step: &Value, kind: &str) -> Result<()> {
        if !self.guards_pass(step)? {
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
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if source.is_dir() {
            if !flag(step, "recursive") {
                bail!("{} is a directory", source.display());
            }
            copy_tree(&source, &target, false)
        } else {
            fs::copy(&source, &target)?;
            Ok(())
        }
    }

    fn move_(&self, step: &Value) -> Result<()> {
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
        fs::rename(&source, &target)?;
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

    fn guards_pass(&self, step: &Value) -> Result<bool> {
        for guard in step
            .get("guards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
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
        copy_tree(&src, &dst, true)?;
        assert_eq!(fs::read_to_string(dst.join("a.cnf"))?, "edited");
        assert_eq!(fs::read_to_string(dst.join("a.cnf.default"))?, "new");
        assert!(!dst.join("same.default").exists());
        assert_eq!(fs::read_to_string(dst.join("sub/b"))?, "b");
        Ok(())
    }
}
