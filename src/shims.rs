use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use eyre::WrapErr;
use itertools::Itertools;
use rayon::prelude::*;

use crate::cli::exec::Exec;
use crate::config::Config;
use crate::env;
use crate::fake_asdf;
use crate::file::{create_dir_all, display_path, remove_all};
use crate::lock_file::LockFile;

use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::{dirs, file};

// executes as if it was a shim if the command is not "rtx", e.g.: "node"
#[allow(dead_code)]
pub fn handle_shim(config: &Config, args: &[String]) -> Result<()> {
    let (_, bin_name) = args[0].rsplit_once('/').unwrap_or(("", &args[0]));
    if bin_name == "rtx" {
        return Ok(());
    }
    let mut args: Vec<OsString> = args.iter().map(OsString::from).collect();
    args[0] = which_shim(config, bin_name)?.into();
    let exec = Exec {
        tool: vec![],
        c: None,
        command: Some(args),
        cd: None,
        jobs: None,
        raw: false,
    };
    exec.run(config)?;
    exit(0);
}

fn which_shim(config: &Config, bin_name: &str) -> Result<PathBuf> {
    let shim = dirs::SHIMS.join(bin_name);
    if shim.exists() {
        let ts = ToolsetBuilder::new().build(config)?;
        if let Some((p, tv)) = ts.which(bin_name) {
            if let Some(bin) = p.which(&tv, bin_name)? {
                return Ok(bin);
            }
        }
        // fallback for "system"
        for path in &*env::PATH {
            if fs::canonicalize(path).unwrap_or_default()
                == fs::canonicalize(&*dirs::SHIMS).unwrap_or_default()
            {
                continue;
            }
            let bin = path.join(bin_name);
            if bin.exists() {
                return Ok(bin);
            }
        }
        let tvs = ts.list_rtvs_with_bin(config, bin_name)?;
        err_no_version_set(config, ts, bin_name, tvs)?;
    }
    Err(eyre!("{} is not a valid shim", bin_name))
}

pub fn reshim(config: &Config, ts: &Toolset) -> Result<()> {
    let _lock = LockFile::new(&dirs::SHIMS)
        .with_callback(|l| {
            trace!("reshim callback {}", l.display());
        })
        .lock();

    let rtx_bin = file::which("rtx").unwrap_or(env::RTX_EXE.clone());

    create_dir_all(&*dirs::SHIMS)?;

    let existing_shims = list_executables_in_dir(&dirs::SHIMS)?
        .into_par_iter()
        .filter(|bin| {
            dirs::SHIMS
                .join(bin)
                .read_link()
                .is_ok_and(|p| p == rtx_bin)
        })
        .collect::<HashSet<_>>();

    let shims: HashSet<String> = ts
        .list_installed_versions(config)?
        .into_par_iter()
        .flat_map(|(t, tv)| {
            list_tool_bins(t.clone(), &tv).unwrap_or_else(|e| {
                warn!("Error listing bin paths for {}: {:#}", tv, e);
                Vec::new()
            })
        })
        .collect();

    let shims_to_add = shims.difference(&existing_shims);
    let shims_to_remove = existing_shims.difference(&shims);

    for shim in shims_to_add {
        let symlink_path = dirs::SHIMS.join(shim);
        file::make_symlink(&rtx_bin, &symlink_path).wrap_err_with(|| {
            eyre!(
                "Failed to create symlink from {} to {}",
                display_path(&rtx_bin),
                display_path(&symlink_path)
            )
        })?;
    }
    for shim in shims_to_remove {
        let symlink_path = dirs::SHIMS.join(shim);
        remove_all(&symlink_path)?;
    }
    for plugin in config.list_plugins() {
        match dirs::PLUGINS.join(plugin.name()).join("shims").read_dir() {
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

// lists all the paths to bins in a tv that shims will be needed for
fn list_tool_bins(t: Arc<dyn Plugin>, tv: &ToolVersion) -> Result<Vec<String>> {
    Ok(t.list_bin_paths(tv)?
        .into_iter()
        .par_bridge()
        .filter(|path| path.exists())
        .map(|dir| list_executables_in_dir(&dir))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn list_executables_in_dir(dir: &Path) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for bin in dir.read_dir()? {
        let bin = bin?;
        // skip non-files and non-symlinks or non-executable files
        if (!bin.file_type()?.is_file() && !bin.file_type()?.is_symlink())
            || !file::is_executable(&bin.path())
        {
            continue;
        }
        out.insert(bin.file_name().into_string().unwrap());
    }
    Ok(out)
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
        rtx x -- {target} "$@"
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

fn err_no_version_set(
    config: &Config,
    ts: Toolset,
    bin_name: &str,
    tvs: Vec<ToolVersion>,
) -> Result<()> {
    if tvs.is_empty() {
        return Ok(());
    }
    let missing_plugins = tvs.iter().map(|tv| &tv.plugin_name).collect::<HashSet<_>>();
    let mut missing_tools = ts
        .list_missing_versions(config)
        .into_iter()
        .filter(|t| missing_plugins.contains(&t.plugin_name))
        .collect_vec();
    if missing_tools.is_empty() {
        let mut msg = format!("No version is set for shim: {}\n", bin_name);
        msg.push_str("Set a global default version with one of the following:\n");
        for tv in tvs {
            msg.push_str(&format!("rtx use -g {}@{}\n", tv.plugin_name, tv.version));
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
        msg.push_str("Install all missing tools with: rtx install\n");
        Err(eyre!(msg.trim().to_string()))
    }
}
