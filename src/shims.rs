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
    if bin_name.starts_with("mise") || cfg!(test) {
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
    };
    time!("shim exec");
    exec.run().await?;
    exit(0);
}

async fn which_shim(config: &mut Arc<Config>, bin_name: &str) -> Result<PathBuf> {
    let mut ts = ToolsetBuilder::new().build(config).await?;
    if let Some((p, tv)) = ts.which(config, bin_name).await {
        if let Some(bin) = p.which(config, &tv, bin_name).await? {
            trace!(
                "shim[{bin_name}] ToolVersion: {tv} bin: {bin}",
                bin = display_path(&bin)
            );
            return Ok(bin);
        }
    }
    if Settings::get().not_found_auto_install && console::user_attended() {
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
    let tvs = ts.list_rtvs_with_bin(config, bin_name).await?;
    err_no_version_set(config, ts, bin_name, tvs).await
}

pub async fn reshim(config: &Arc<Config>, ts: &Toolset, force: bool) -> Result<()> {
    let _lock = LockFile::new(&dirs::SHIMS)
        .with_callback(|l| {
            trace!("reshim callback {}", l.display());
        })
        .lock();

    let mise_bin = file::which("mise").unwrap_or(env::MISE_BIN.clone());
    let mise_bin = mise_bin.absolutize()?; // relative paths don't work as shims

    if force {
        file::remove_all(*dirs::SHIMS)?;
    }
    file::create_dir_all(*dirs::SHIMS)?;

    let (shims_to_add, shims_to_remove) = get_shim_diffs(config, &mise_bin, ts).await?;

    for shim in shims_to_add {
        let symlink_path = dirs::SHIMS.join(&shim);
        add_shim(&mise_bin, &symlink_path, &shim)?;
    }
    for shim in shims_to_remove {
        let symlink_path = dirs::SHIMS.join(shim);
        file::remove_all(&symlink_path)?;
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

#[cfg(windows)]
fn add_shim(mise_bin: &Path, symlink_path: &Path, shim: &str) -> Result<()> {
    match Settings::get().windows_shim_mode.as_ref() {
        "file" => {
            let shim = shim.trim_end_matches(".cmd");
            // write a shim file without extension for use in Git Bash/Cygwin
            file::write(
                symlink_path.with_extension(""),
                formatdoc! {r#"
        #!/bin/bash

        exec mise x -- {shim} "$@"
        "#},
            )
            .wrap_err_with(|| {
                eyre!(
                    "Failed to create symlink from {} to {}",
                    display_path(mise_bin),
                    display_path(symlink_path)
                )
            })?;
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
        get_desired_shims(config, toolset)
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

fn list_shims() -> Result<HashSet<String>> {
    Ok(dirs::SHIMS
        .read_dir()?
        .map(|bin| {
            let bin = bin?;
            // files and symlinks which are executable or extensionless files (Git Bash/Cygwin)
            if (file::is_executable(&bin.path()) || bin.path().extension().is_none())
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

async fn get_desired_shims(config: &Arc<Config>, toolset: &Toolset) -> Result<HashSet<String>> {
    let mut shims = HashSet::new();
    for (t, tv) in toolset.list_installed_versions(config).await? {
        let bins = list_tool_bins(config, t.clone(), &tv)
            .await
            .unwrap_or_else(|e| {
                warn!("Error listing bin paths for {}: {:#}", tv, e);
                Vec::new()
            });
        if cfg!(windows) {
            shims.extend(bins.into_iter().flat_map(|b| {
                let p = PathBuf::from(&b);
                match Settings::get().windows_shim_mode.as_ref() {
                    "hardlink" | "symlink" => {
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
            // some bins might be uppercased but on mac APFS is case insensitive
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
    if shim.exists() {
        file::remove_file_async(shim).await?;
    }
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
