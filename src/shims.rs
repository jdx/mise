use crate::exit;
use std::collections::{BTreeSet, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backend::Backend;
use crate::cli::exec::Exec;
use crate::config::{CONFIG, SETTINGS};
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::{backend, dirs, env, fake_asdf, file, logger};
use color_eyre::eyre::{bail, eyre, Result};
use eyre::WrapErr;
use indoc::formatdoc;
use itertools::Itertools;
use rayon::prelude::*;

// executes as if it was a shim if the command is not "mise", e.g.: "node"
pub fn handle_shim() -> Result<()> {
    // TODO: instead, check if bin is in shims dir
    let bin_name = *env::MISE_BIN_NAME;
    if bin_name.starts_with("mise") || cfg!(test) {
        return Ok(());
    }
    logger::init();
    let args = env::ARGS.read().unwrap();
    trace!("shim[{bin_name}] args: {}", args.join(" "));
    let mut args: Vec<OsString> = args.iter().map(OsString::from).collect();
    args[0] = which_shim(&env::MISE_BIN_NAME)?.into();
    env::set_var("__MISE_SHIM", "1");
    let exec = Exec {
        tool: vec![],
        c: None,
        command: Some(args),
        jobs: None,
        raw: false,
    };
    exec.run()?;
    exit(0);
}

fn which_shim(bin_name: &str) -> Result<PathBuf> {
    let mut ts = ToolsetBuilder::new().build(&CONFIG)?;
    if let Some((p, tv)) = ts.which(bin_name) {
        if let Some(bin) = p.which(&tv, bin_name)? {
            trace!(
                "shim[{bin_name}] ToolVersion: {tv} bin: {bin}",
                bin = display_path(&bin)
            );
            return Ok(bin);
        }
    }
    if SETTINGS.not_found_auto_install && console::user_attended() {
        for tv in ts.install_missing_bin(bin_name)?.unwrap_or_default() {
            let p = tv.get_backend();
            if let Some(bin) = p.which(&tv, bin_name)? {
                trace!(
                    "shim[{bin_name}] NOT_FOUND ToolVersion: {tv} bin: {bin}",
                    bin = display_path(&bin)
                );
                return Ok(bin);
            }
        }
    }
    // fallback for "system"
    for path in &*env::PATH {
        if fs::canonicalize(path).unwrap_or_default()
            == fs::canonicalize(*dirs::SHIMS).unwrap_or_default()
        {
            continue;
        }
        let bin = path.join(bin_name);
        if bin.exists() {
            trace!("shim[{bin_name}] SYSTEM {bin}", bin = display_path(&bin));
            return Ok(bin);
        }
    }
    let tvs = ts.list_rtvs_with_bin(bin_name)?;
    err_no_version_set(ts, bin_name, tvs)
}

pub fn reshim(ts: &Toolset, force: bool) -> Result<()> {
    let _lock = LockFile::new(&dirs::SHIMS)
        .with_callback(|l| {
            trace!("reshim callback {}", l.display());
        })
        .lock();

    let mise_bin = file::which("mise").unwrap_or(env::MISE_BIN.clone());

    if force {
        file::remove_all(*dirs::SHIMS)?;
    }
    file::create_dir_all(*dirs::SHIMS)?;

    let (shims_to_add, shims_to_remove) = get_shim_diffs(&mise_bin, ts)?;

    for shim in shims_to_add {
        let symlink_path = dirs::SHIMS.join(&shim);
        add_shim(&mise_bin, &symlink_path, &shim)?;
    }
    for shim in shims_to_remove {
        let symlink_path = dirs::SHIMS.join(shim);
        file::remove_all(&symlink_path)?;
    }
    for plugin in backend::list() {
        match dirs::PLUGINS.join(plugin.id()).join("shims").read_dir() {
            Ok(files) => {
                for bin in files {
                    let bin = bin?;
                    let bin_name = bin.file_name().into_string().unwrap();
                    let symlink_path = dirs::SHIMS.join(bin_name);
                    make_shim(&bin.path(), &symlink_path)?;
                }
            }
            Err(_) => {
                continue;
            }
        }
    }

    Ok(())
}

#[cfg(windows)]
fn add_shim(mise_bin: &Path, symlink_path: &Path, shim: &str) -> Result<()> {
    let shim = shim.trim_end_matches(".cmd");
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
pub fn get_shim_diffs(
    mise_bin: impl AsRef<Path>,
    toolset: &Toolset,
) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mise_bin = mise_bin.as_ref();
    let (actual_shims, desired_shims) =
        rayon::join(|| get_actual_shims(mise_bin), || get_desired_shims(toolset));
    let (actual_shims, desired_shims) = (actual_shims?, desired_shims?);
    let out: (BTreeSet<String>, BTreeSet<String>) = (
        desired_shims.difference(&actual_shims).cloned().collect(),
        actual_shims.difference(&desired_shims).cloned().collect(),
    );
    time!("get_shim_diffs sizes: ({},{})", out.0.len(), out.1.len());
    Ok(out)
}

fn get_actual_shims(mise_bin: impl AsRef<Path>) -> Result<HashSet<String>> {
    let mise_bin = mise_bin.as_ref();

    Ok(list_executables_in_dir(&dirs::SHIMS)?
        .into_par_iter()
        .filter(|bin| {
            let path = dirs::SHIMS.join(bin);

            !path.is_symlink() || path.read_link().is_ok_and(|p| p == mise_bin)
        })
        .collect::<HashSet<_>>())
}

fn list_executables_in_dir(dir: &Path) -> Result<HashSet<String>> {
    Ok(dir
        .read_dir()?
        .par_bridge()
        .map(|bin| {
            let bin = bin?;
            // files and symlinks which are executable
            if file::is_executable(&bin.path())
                && (bin.file_type()?.is_file() || bin.file_type()?.is_symlink())
            {
                Ok(Some(bin.file_name().into_string().unwrap()))
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn get_desired_shims(toolset: &Toolset) -> Result<HashSet<String>> {
    Ok(toolset
        .list_installed_versions()?
        .into_par_iter()
        .flat_map(|(t, tv)| {
            let bins = list_tool_bins(t.clone(), &tv).unwrap_or_else(|e| {
                warn!("Error listing bin paths for {}: {:#}", tv, e);
                Vec::new()
            });
            if cfg!(windows) {
                bins.into_iter()
                    .map(|b| {
                        let p = PathBuf::from(&b);
                        p.with_extension("cmd").to_string_lossy().to_string()
                    })
                    .collect()
            } else {
                bins
            }
        })
        .collect())
}

// lists all the paths to bins in a tv that shims will be needed for
fn list_tool_bins(t: Arc<dyn Backend>, tv: &ToolVersion) -> Result<Vec<String>> {
    Ok(t.list_bin_paths(tv)?
        .into_iter()
        .par_bridge()
        .filter(|p| p.parent().is_some())
        .filter(|path| path.exists())
        .map(|dir| list_executables_in_dir(&dir))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn make_shim(target: &Path, shim: &Path) -> Result<()> {
    if shim.exists() {
        file::remove_file(shim)?;
    }
    file::write(
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
    )?;
    file::make_executable(shim)?;
    trace!(
        "shim created from {} to {}",
        target.display(),
        shim.display()
    );
    Ok(())
}

fn err_no_version_set(ts: Toolset, bin_name: &str, tvs: Vec<ToolVersion>) -> Result<PathBuf> {
    if tvs.is_empty() {
        bail!("{} is not a valid shim", bin_name);
    }
    let missing_plugins = tvs.iter().map(|tv| tv.backend()).collect::<HashSet<_>>();
    let mut missing_tools = ts
        .list_missing_versions()
        .into_iter()
        .filter(|t| missing_plugins.contains(t.backend()))
        .collect_vec();
    if missing_tools.is_empty() {
        let mut msg = format!("No version is set for shim: {}\n", bin_name);
        msg.push_str("Set a global default version with one of the following:\n");
        for tv in tvs {
            msg.push_str(&format!("mise use -g {}@{}\n", tv.backend(), tv.version));
        }
        Err(eyre!(msg.trim().to_string()))
    } else {
        let mut msg = format!(
            "Tool{} not installed for shim: {}\n",
            if missing_tools.len() > 1 { "s" } else { "" },
            bin_name
        );
        for t in missing_tools.drain(..) {
            msg.push_str(&format!("Missing tool version: {}\n", t));
        }
        msg.push_str("Install all missing tools with: mise install\n");
        Err(eyre!(msg.trim().to_string()))
    }
}
