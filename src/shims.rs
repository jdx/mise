use crate::exit;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{
    collections::{BTreeSet, HashSet},
    sync::atomic::Ordering,
};

use crate::backend::Backend;
use crate::cli::exec::Exec;
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::{backend, dirs, env, fake_asdf, file};
use color_eyre::eyre::{Result, bail, eyre};
use eyre::WrapErr;
use indoc::formatdoc;
use itertools::Itertools;
use path_absolutize::Absolutize;
use tokio::task::JoinSet;

// executes as if it was a shim if the command is not "mise", e.g.: "node"
pub async fn handle_shim() -> Result<()> {
    // TODO: instead, check if bin is in shims dir
    let bin_name = *env::MISE_BIN_NAME;
    if env::is_mise_binary(bin_name) || cfg!(test) {
        return Ok(());
    }
    let mut config = Config::get().await?;
    let mut args = env::ARGS.read().unwrap().clone();
    env::PREFER_OFFLINE.store(true, Ordering::Relaxed);
    trace!("shim[{bin_name}] args: {}", args.join(" "));
    args[0] = which_shim(&mut config, &env::MISE_BIN_NAME)
        .await?
        .to_string_lossy()
        .to_string();
    env::set_var("__MISE_SHIM", "1");
    let exec = Exec {
        tool: vec![],
        c: None,
        command: Some(args),
        jobs: None,
        raw: false,
        no_deps: true, // Skip deps for shims to avoid performance impact
        fresh_env: false,
        deny_all: false,
        deny_read: false,
        deny_write: false,
        deny_net: false,
        deny_env: false,
        allow_read: vec![],
        allow_write: vec![],
        allow_net: vec![],
        allow_env: vec![],
    };
    time!("shim exec");
    exec.run().await?;
    exit(0);
}

async fn which_shim(config: &mut Arc<Config>, bin_name: &str) -> Result<PathBuf> {
    let mut ts = ToolsetBuilder::new().build(config).await?;
    if let Some((p, tv)) = ts.which(config, bin_name).await
        && let Some(bin) = p.which(config, &tv, bin_name).await?
    {
        trace!(
            "shim[{bin_name}] ToolVersion: {tv} bin: {bin}",
            bin = display_path(&bin)
        );
        return Ok(bin);
    }
    if Settings::get().not_found_auto_install {
        for tv in ts
            .install_missing_bin(config, bin_name)
            .await?
            .unwrap_or_default()
        {
            let p = tv.backend()?;
            if let Some(bin) = p.which(config, &tv, bin_name).await? {
                trace!(
                    "shim[{bin_name}] NOT_FOUND ToolVersion: {tv} bin: {bin}",
                    bin = display_path(&bin)
                );
                return Ok(bin);
            }
        }
    }
    // fallback for "system"
    let mise_bin = file::canonicalize_or_self(&env::MISE_BIN);
    let user_shims = file::canonicalize_cached(&dirs::SHIMS);
    let sys_shims = {
        let p = env::MISE_SYSTEM_DATA_DIR.join("shims");
        file::canonicalize_cached(&p)
    };
    for path in &*env::PATH {
        if let Some(canon_path) = file::canonicalize_cached(path)
            && (user_shims.as_ref() == Some(&canon_path) || sys_shims.as_ref() == Some(&canon_path))
        {
            continue;
        }
        let bin = path.join(bin_name);
        if bin.exists() {
            // Skip if this binary is a mise shim (symlink pointing to the mise binary)
            if file::canonicalize_cached(&bin).is_some_and(|bin| bin == mise_bin) {
                continue;
            }
            trace!("shim[{bin_name}] SYSTEM {bin}", bin = display_path(&bin));
            return Ok(bin);
        }
    }
    let tvs = ts.list_rtvs_with_bin(config, bin_name).await?;
    err_no_version_set(config, ts, bin_name, tvs).await
}

pub async fn reshim(config: &Arc<Config>, ts: &Toolset, force: bool) -> Result<()> {
    let _lock = LockFile::new(&dirs::SHIMS)
        .with_callback(|l| {
            trace!("reshim callback {}", l.display());
        })
        .lock();

    let mise_bin = file::which_no_shims("mise").unwrap_or(env::MISE_BIN.clone());
    let mise_bin = mise_bin.absolutize()?; // relative paths don't work as shims

    #[cfg(windows)]
    let shim_mode = effective_shim_mode(&mise_bin);
    #[cfg(not(windows))]
    let shim_mode = String::new();
    let shim_mode_changed = cfg!(windows) && {
        let mode_file = dirs::SHIMS.join(".mode");
        mode_file
            .exists()
            .then(|| fs::read_to_string(&mode_file).unwrap_or_default())
            .is_some_and(|prev| prev.trim() != shim_mode)
    };
    // On Windows, "exe"/"hardlink" shims are literal copies of the mise(-shim)
    // binary, so they go stale when mise is updated (by self-update or an
    // external package manager) until a forced reshim. Track the mise version
    // that generated the shims in a `.version` marker (mirroring `.mode`) and
    // rebuild from scratch whenever it changes. The marker is written by
    // whichever binary runs reshim, so after an update the new binary stamps
    // the new version. See discussion #10022.
    let shim_version = env!("CARGO_PKG_VERSION");
    let shim_version_changed = cfg!(windows) && {
        let version_file = dirs::SHIMS.join(".version");
        let prev = fs::read_to_string(&version_file).ok();
        shim_version_stale(prev.as_deref(), shim_version, &shim_mode)
    };
    if force || shim_mode_changed || shim_version_changed {
        // On Windows, .exe shims may be locked by processes or the shell (they
        // are on PATH).  Instead of removing the entire directory (which fails
        // with "Access is denied"), remove individual files with a rename-first
        // fallback so locked executables are moved out of the way.
        if cfg!(windows) {
            remove_shims_individually(&dirs::SHIMS)?;
        } else {
            file::remove_all(*dirs::SHIMS)?;
        }
    }
    file::create_dir_all(*dirs::SHIMS)?;
    if cfg!(windows) {
        let mode_file = dirs::SHIMS.join(".mode");
        file::write(&mode_file, &shim_mode)?;
        // Written for every shim mode (like `.mode`) even though it is only
        // consulted for "exe"/"hardlink" modes; for "file"/"symlink" it is
        // harmless and keeps the marker current if the mode later changes
        // (mode transitions themselves are handled by `shim_mode_changed`).
        let version_file = dirs::SHIMS.join(".version");
        file::write(&version_file, shim_version)?;
    }

    let (shims_to_add, shims_to_remove) = if force || shim_mode_changed || shim_version_changed {
        // After a full wipe, all desired shims need to be re-created.
        let desired = get_desired_shims(config, &mise_bin, ts).await?;
        (
            desired.into_iter().collect::<BTreeSet<_>>(),
            BTreeSet::new(),
        )
    } else {
        get_shim_diffs(config, &mise_bin, ts).await?
    };

    for shim in shims_to_add {
        let symlink_path = dirs::SHIMS.join(&shim);
        // On Windows, remove the old shim first (with rename fallback for
        // locked .exe files) so the new one can be written.
        if cfg!(windows) && symlink_path.exists() {
            remove_shim_with_rename_fallback(&symlink_path)?;
        }
        add_shim(&mise_bin, &symlink_path, &shim)?;
    }
    for shim in shims_to_remove {
        let symlink_path = dirs::SHIMS.join(shim);
        if cfg!(windows) {
            remove_shim_with_rename_fallback(&symlink_path)?;
        } else {
            file::remove_all(&symlink_path)?;
        }
    }
    let mut jset = JoinSet::new();
    for plugin in backend::list() {
        jset.spawn(async move {
            if let Ok(files) = dirs::PLUGINS.join(plugin.id()).join("shims").read_dir() {
                for bin in files {
                    let bin = bin?;
                    let bin_name = bin.file_name().into_string().unwrap();
                    let symlink_path = dirs::SHIMS.join(bin_name);
                    make_shim(&bin.path(), &symlink_path).await?;
                }
            }
            Ok(())
        });
    }
    jset.join_all()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    Ok(())
}

/// Remove all shim files from a directory individually, skipping dotfiles like
/// `.mode`. Uses [`remove_shim_with_rename_fallback`] for each entry so locked
/// `.exe` files on Windows are renamed out of the way instead of causing a
/// hard error.
fn remove_shims_individually(shims_dir: &Path) -> Result<()> {
    let entries = match shims_dir.read_dir() {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(e).wrap_err_with(|| {
                format!(
                    "failed to read shims directory: {}",
                    display_path(shims_dir)
                )
            });
        }
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        // skip dotfiles (e.g. .mode) — these are metadata, not shims
        if is_hidden_shim_name(&name) {
            continue;
        }
        let path = entry.path();
        remove_shim_with_rename_fallback(&path)?;
    }
    Ok(())
}

/// Remove a single shim file. On Windows, if deletion fails (e.g. because the
/// `.exe` is locked by another process), rename it to `<name>.old` so the path
/// is freed for a new shim. The `.old` file will be cleaned up on the next
/// reshim or when the lock is released.
fn remove_shim_with_rename_fallback(path: &Path) -> Result<()> {
    // First, try to clean up any leftover .old files from a previous run.
    let old_path = path.with_extension("old");
    if old_path.exists() {
        let _ = fs::remove_file(&old_path); // best-effort
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5) | Some(32)) => {
            // ERROR_ACCESS_DENIED (5) or ERROR_SHARING_VIOLATION (32): file is
            // locked by another process, rename it instead.
            trace!(
                "cannot delete locked shim {}, renaming to .old",
                display_path(path)
            );
            fs::rename(path, &old_path).wrap_err_with(|| {
                format!(
                    "failed to rename locked shim {} to {}",
                    display_path(path),
                    display_path(&old_path)
                )
            })?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).wrap_err_with(|| format!("failed to remove shim: {}", display_path(path))),
    }
}

#[cfg(windows)]
fn find_mise_shim_bin(mise_bin: &Path) -> Option<PathBuf> {
    // Look next to the mise binary first
    if let Some(parent) = mise_bin.parent() {
        let candidate = parent.join("mise-shim.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // Fall back to searching PATH
    // Note: file::which on Windows checks extension only, not file existence,
    // so we must verify the file actually exists.
    file::which("mise-shim.exe").filter(|p| p.is_file())
}

/// Resolve the effective Windows shim mode, falling back to "file" if "exe" is
/// requested but mise-shim.exe is not available.
#[cfg(windows)]
fn effective_shim_mode(mise_bin: &Path) -> String {
    let mode = Settings::get().windows_shim_mode.clone();
    if mode == "exe" && find_mise_shim_bin(mise_bin).is_none() {
        warn!(
            "mise-shim.exe not found next to {} or on PATH, falling back to \"file\" shim mode",
            display_path(mise_bin)
        );
        return "file".to_string();
    }
    mode
}

/// Build the extension-less bash shim used on Windows in "file" mode (for Git
/// Bash/Cygwin). "exe" mode does not emit this — its native <tool>.exe is found
/// by those shells via `.exe` magic — so only "file" mode reaches this code.
///
/// The shim's directory can leak into WSL via the default Windows-PATH interop
/// (it is mounted under /mnt/c where every file is treated as executable), so WSL
/// runs this script natively. Calling the Windows `mise` from there either fails
/// with `exec: mise: not found` or, with a Linux mise present, recurses forever --
/// mise's loop guard only recognises its own shims dir, not the Windows one under
/// /mnt/c. So detect WSL, drop this shim's own directory from PATH, and exec a
/// native tool instead (or fail with a clear `<tool>: not found`). Outside WSL the
/// guard is inert, so Git Bash/Cygwin behaviour is unchanged. (#10299)
#[cfg(windows)]
fn bash_shim_script(tool: &str) -> String {
    formatdoc! {r#"
        #!/bin/bash

        if [ -n "${{WSL_DISTRO_NAME:-}}" ] || [ -n "${{WSL_INTEROP:-}}" ] || [ -e /proc/sys/fs/binfmt_misc/WSLInterop ]; then
          shim_dir=$(cd -- "$(dirname -- "$0")" && pwd -P)
          new_path=
          # disable globbing so a PATH entry containing * ? [ is not expanded
          set -f
          IFS=:
          for p in $PATH; do
            [ "$p" = "$shim_dir" ] && continue
            new_path="${{new_path:+$new_path:}}$p"
          done
          unset IFS
          set +f
          export PATH="$new_path"
          exec {tool} "$@"
        fi

        exec mise x -- {tool} "$@"
        "#}
}

#[cfg(windows)]
fn add_shim(mise_bin: &Path, symlink_path: &Path, shim: &str) -> Result<()> {
    match effective_shim_mode(mise_bin).as_ref() {
        "exe" => {
            // In "exe" mode every desired shim is a native <tool>.exe copy of
            // mise-shim.exe (see get_desired_shims). No extension-less bash shim is
            // emitted: Git Bash / Cygwin resolve a bare name to the .exe via their
            // `.exe` magic, so emitting one is redundant and only pollutes WSL via
            // /mnt/c PATH interop (#10299).
            let mise_shim_bin =
                find_mise_shim_bin(mise_bin).ok_or_else(|| eyre!("mise-shim.exe not found"))?;
            // Copy mise-shim.exe as <tool>.exe
            fs::copy(&mise_shim_bin, symlink_path).wrap_err_with(|| {
                eyre!(
                    "Failed to copy {} to {}",
                    display_path(&mise_shim_bin),
                    display_path(symlink_path)
                )
            })?;
            Ok(())
        }
        "file" => {
            let shim = shim.trim_end_matches(".cmd");
            // write a shim file without extension for use in Git Bash/Cygwin
            file::write(symlink_path.with_extension(""), bash_shim_script(shim)).wrap_err_with(
                || {
                    eyre!(
                        "Failed to create symlink from {} to {}",
                        display_path(mise_bin),
                        display_path(symlink_path)
                    )
                },
            )?;
            file::write(
                symlink_path.with_extension("cmd"),
                formatdoc! {r#"
        @echo off
        setlocal
        mise x -- {shim} %*
        "#},
            )
            .wrap_err_with(|| {
                eyre!(
                    "Failed to create symlink from {} to {}",
                    display_path(mise_bin),
                    display_path(symlink_path)
                )
            })
        }
        "hardlink" => fs::hard_link(mise_bin, symlink_path).wrap_err_with(|| {
            eyre!(
                "Failed to create hardlink from {} to {}",
                display_path(mise_bin),
                display_path(symlink_path)
            )
        }),
        "symlink" => {
            std::os::windows::fs::symlink_file(mise_bin, symlink_path).wrap_err_with(|| {
                eyre!(
                    "Failed to create symlink from {} to {}",
                    display_path(mise_bin),
                    display_path(symlink_path)
                )
            })
        }
        _ => panic!("Unknown shim mode"),
    }
}

#[cfg(unix)]
fn add_shim(mise_bin: &Path, symlink_path: &Path, _shim: &str) -> Result<()> {
    file::make_symlink(mise_bin, symlink_path).wrap_err_with(|| {
        eyre!(
            "Failed to create symlink from {} to {}",
            display_path(mise_bin),
            display_path(symlink_path)
        )
    })?;
    Ok(())
}

// get_shim_diffs contrasts the actual shims on disk
// with the desired shims specified by the Toolset
// and returns a tuple of (missing shims, extra shims)
pub async fn get_shim_diffs(
    config: &Arc<Config>,
    mise_bin: impl AsRef<Path>,
    toolset: &Toolset,
) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mise_bin = mise_bin.as_ref();
    let (actual_shims, desired_shims) = tokio::join!(
        get_actual_shims(mise_bin),
        get_desired_shims(config, mise_bin, toolset)
    );
    let (actual_shims, desired_shims) = (actual_shims?, desired_shims?);
    let out: (BTreeSet<String>, BTreeSet<String>) = (
        desired_shims.difference(&actual_shims).cloned().collect(),
        actual_shims.difference(&desired_shims).cloned().collect(),
    );
    time!("get_shim_diffs sizes: ({},{})", out.0.len(), out.1.len());
    Ok(out)
}

async fn get_actual_shims(mise_bin: impl AsRef<Path>) -> Result<HashSet<String>> {
    let mise_bin = mise_bin.as_ref();

    Ok(list_shims()?
        .into_iter()
        .filter(|bin| {
            let path = dirs::SHIMS.join(bin);

            !path.is_symlink() || path.read_link().is_ok_and(|p| p == mise_bin)
        })
        .collect::<HashSet<_>>())
}

fn list_executables_in_dir(dir: &Path) -> Result<HashSet<String>> {
    Ok(dir
        .read_dir()?
        .map(|bin| {
            let bin = bin?;
            let name = bin.file_name();
            if is_hidden_shim_name(&name) {
                return Ok(None);
            }
            // files and symlinks which are executable
            if file::is_executable(&bin.path())
                && (bin.file_type()?.is_file() || bin.file_type()?.is_symlink())
            {
                Ok(name.into_string().ok())
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn list_shims() -> Result<HashSet<String>> {
    Ok(dirs::SHIMS
        .read_dir()?
        .map(|bin| {
            let bin = bin?;
            let name = bin.file_name();
            // skip dotfiles (e.g. .mode) — these are metadata, not shims
            if is_hidden_shim_name(&name) {
                return Ok(None);
            }
            // files and symlinks which are executable or extensionless files (Git Bash/Cygwin)
            if (file::is_executable(&bin.path()) || bin.path().extension().is_none())
                && (bin.file_type()?.is_file() || bin.file_type()?.is_symlink())
            {
                Ok(name.into_string().ok())
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn is_hidden_shim_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// Whether existing shims were generated by a different mise version AND the
/// current shim mode produces version-dependent shim files. "exe"/"hardlink"
/// embed a literal copy of the mise/mise-shim binary; "file" writes a bash script
/// whose contents are baked into the mise binary as well (e.g. the WSL guard added
/// in #10299), so all three must rebuild on a version change to pick up script
/// changes — otherwise a normal reshim leaves the old script in place. "symlink"
/// only points at the mise binary (no embedded content), so it is never
/// version-stale. `prev == None` (no `.version` marker yet) heals installs that
/// predate the marker by forcing a one-time rebuild. See discussions #10022 and
/// #10299.
fn shim_version_stale(prev: Option<&str>, current: &str, shim_mode: &str) -> bool {
    if !matches!(shim_mode, "exe" | "hardlink" | "file") {
        return false;
    }
    prev.map(|p| p.trim() != current).unwrap_or(true)
}

async fn get_desired_shims(
    config: &Arc<Config>,
    mise_bin: &Path,
    toolset: &Toolset,
) -> Result<HashSet<String>> {
    let _mise_bin = mise_bin; // used on Windows only
    let mut shims = HashSet::new();
    for (t, tv) in toolset.list_installed_versions(config).await? {
        let bins = list_tool_bins(config, t.clone(), &tv)
            .await
            .unwrap_or_else(|e| {
                warn!("Error listing bin paths for {}: {:#}", tv, e);
                Vec::new()
            });
        if cfg!(windows) {
            #[cfg(windows)]
            let shim_mode = effective_shim_mode(_mise_bin);
            #[cfg(not(windows))]
            let shim_mode = String::new();
            shims.extend(bins.into_iter().flat_map(|b| {
                let p = PathBuf::from(&b);
                match shim_mode.as_ref() {
                    "hardlink" | "symlink" => {
                        vec![p.with_extension("exe").to_string_lossy().to_string()]
                    }
                    "exe" => {
                        // Only the native <tool>.exe is needed. Git Bash / Cygwin /
                        // MSYS2 resolve a bare `tool` to `tool.exe` via their `.exe`
                        // magic, and mise-shim.exe derives the tool from its own file
                        // name, so it runs correctly however it is invoked. We do NOT
                        // emit an extension-less bash shim here: that variant is only
                        // required in "file" mode (no .exe, and Cygwin won't auto-append
                        // .cmd) and is what leaked into WSL via /mnt/c PATH interop
                        // (#10299).
                        vec![p.with_extension("exe").to_string_lossy().to_string()]
                    }
                    "file" => {
                        vec![
                            p.with_extension("").to_string_lossy().to_string(),
                            p.with_extension("cmd").to_string_lossy().to_string(),
                        ]
                    }
                    _ => panic!("Unknown shim mode"),
                }
            }));
        } else if cfg!(macos) {
            // some bins might be uppercased but on mac APFS is case-insensitive
            shims.extend(bins.into_iter().map(|b| b.to_lowercase()));
        } else {
            shims.extend(bins);
        }
    }
    Ok(shims)
}

// lists all the paths to bins in a tv that shims will be needed for
async fn list_tool_bins(
    config: &Arc<Config>,
    t: Arc<dyn Backend>,
    tv: &ToolVersion,
) -> Result<Vec<String>> {
    Ok(t.list_bin_paths(config, tv)
        .await?
        .into_iter()
        .filter(|p| p.parent().is_some())
        .filter(|path| path.exists())
        .map(|dir| list_executables_in_dir(&dir))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

async fn make_shim(target: &Path, shim: &Path) -> Result<()> {
    file::remove_file_async_if_exists(shim).await?;
    file::write_async(
        shim,
        formatdoc! {r#"
        #!/bin/sh
        export ASDF_DATA_DIR={data_dir}
        export PATH="{fake_asdf_dir}:$PATH"
        mise x -- {target} "$@"
        "#,
        data_dir = dirs::DATA.display(),
        fake_asdf_dir = fake_asdf::setup()?.display(),
        target = target.display()},
    )
    .await?;
    file::make_executable_async(shim).await?;
    trace!(
        "shim created from {} to {}",
        target.display(),
        shim.display()
    );
    Ok(())
}

async fn err_no_version_set(
    config: &Arc<Config>,
    ts: Toolset,
    bin_name: &str,
    tvs: Vec<ToolVersion>,
) -> Result<PathBuf> {
    if tvs.is_empty() {
        bail!(
            "{bin_name} is not a valid shim. This likely means you uninstalled a tool and the shim does not point to anything. Run `mise use <TOOL>` to reinstall the tool."
        );
    }
    let missing_plugins = tvs.iter().map(|tv| tv.ba()).collect::<HashSet<_>>();
    let mut missing_tools = ts
        .list_missing_versions(config)
        .await
        .into_iter()
        .filter(|t| missing_plugins.contains(t.ba()))
        .collect_vec();
    if missing_tools.is_empty() {
        let mut msg = format!("No version is set for shim: {bin_name}\n");
        msg.push_str("Set a global default version with one of the following:\n");
        for tv in tvs {
            msg.push_str(&format!("mise use -g {}@{}\n", tv.ba(), tv.version));
        }
        Err(eyre!(msg.trim().to_string()))
    } else {
        let mut msg = format!(
            "Tool{} not installed for shim: {}\n",
            if missing_tools.len() > 1 { "s" } else { "" },
            bin_name
        );
        for t in missing_tools.drain(..) {
            msg.push_str(&format!("Missing tool version: {t}\n"));
        }
        msg.push_str("Install all missing tools with: mise install\n");
        Err(eyre!(msg.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn bash_shim_script_includes_wsl_guard() {
        let script = bash_shim_script("gh");
        assert!(script.starts_with("#!/bin/bash"));
        // WSL detection
        assert!(script.contains("WSL_DISTRO_NAME"));
        assert!(script.contains("WSL_INTEROP"));
        assert!(script.contains("/proc/sys/fs/binfmt_misc/WSLInterop"));
        assert!(script.contains(r#"shim_dir=$(cd -- "$(dirname -- "$0")" && pwd -P)"#));
        // globbing disabled while splitting PATH so wildcard entries are not expanded
        assert!(script.contains("set -f"));
        // In WSL: drop the shim dir and run the native tool directly.
        assert!(script.contains(r#"exec gh "$@""#));
        // Outside WSL: defer to mise as before.
        assert!(script.contains(r#"exec mise x -- gh "$@""#));
    }

    #[test]
    fn list_executables_in_dir_skips_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        let visible_name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let visible = dir.path().join(visible_name);
        let hidden = dir.path().join(".librsvg-post-link.exe");

        fs::write(&visible, "").unwrap();
        fs::write(&hidden, "").unwrap();
        file::make_executable(&visible).unwrap();
        file::make_executable(&hidden).unwrap();

        let bins = list_executables_in_dir(dir.path()).unwrap();

        assert!(bins.contains(visible_name));
        assert!(!bins.contains(".librsvg-post-link.exe"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn list_executables_in_dir_skips_non_utf8_names() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let dir = tempfile::tempdir().unwrap();
        let non_utf8 = dir.path().join(OsString::from_vec(vec![0xff]));

        fs::write(&non_utf8, "").unwrap();
        file::make_executable(&non_utf8).unwrap();

        let bins = list_executables_in_dir(dir.path()).unwrap();

        assert!(bins.is_empty());
    }

    #[test]
    fn shim_version_stale_detects_version_changes() {
        // exe/hardlink copies embed the binary: a version change makes them stale
        assert!(shim_version_stale(Some("2026.5.13"), "2026.5.16", "exe"));
        assert!(shim_version_stale(
            Some("2026.5.13"),
            "2026.5.16",
            "hardlink"
        ));
        // file mode writes a versioned bash script (e.g. the WSL guard, #10299),
        // so a version change must rebuild it too
        assert!(shim_version_stale(Some("2026.5.13"), "2026.5.16", "file"));
        // matching version is not stale
        assert!(!shim_version_stale(Some("2026.5.16"), "2026.5.16", "exe"));
        assert!(!shim_version_stale(Some("2026.5.16"), "2026.5.16", "file"));
        // surrounding whitespace in the marker is ignored
        assert!(!shim_version_stale(Some("2026.5.16\n"), "2026.5.16", "exe"));
        // no marker yet: heal once (covers installs created before this marker)
        assert!(shim_version_stale(None, "2026.5.16", "exe"));
        assert!(shim_version_stale(None, "2026.5.16", "file"));
        // symlink shims only point at the mise binary, so never version-stale
        assert!(!shim_version_stale(
            Some("2026.5.13"),
            "2026.5.16",
            "symlink"
        ));
    }
}
