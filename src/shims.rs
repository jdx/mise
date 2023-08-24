use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use color_eyre::eyre::{eyre, Result};
use indoc::formatdoc;
use itertools::Itertools;
use rayon::prelude::*;

use crate::cli::command::Command;
use crate::cli::exec::Exec;
use crate::config::Config;
use crate::env;
use crate::fake_asdf;
use crate::file::{create_dir_all, remove_all};
use crate::lock_file::LockFile;
use crate::output::Output;
use crate::tool::Tool;
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::{dirs, file};

// executes as if it was a shim if the command is not "rtx", e.g.: "node"
#[allow(dead_code)]
pub fn handle_shim(mut config: Config, args: &[String], out: &mut Output) -> Result<Config> {
    let (_, bin_name) = args[0].rsplit_once('/').unwrap_or(("", &args[0]));
    if bin_name == "rtx" {
        return Ok(config);
    }
    let mut args: Vec<OsString> = args.iter().map(OsString::from).collect();
    args[0] = which_shim(&mut config, bin_name)?.into();
    let exec = Exec {
        tool: vec![],
        c: None,
        command: Some(args),
        cd: None,
    };
    exec.run(config, out)?;
    exit(0);
}

fn which_shim(config: &mut Config, bin_name: &str) -> Result<PathBuf> {
    let shim = dirs::SHIMS.join(bin_name);
    if shim.exists() {
        let ts = ToolsetBuilder::new().build(config)?;
        if let Some((p, tv)) = ts.which(config, bin_name) {
            if let Some(bin) = p.which(config, &tv, bin_name)? {
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
    let existing_shims = list_executables_in_dir(&dirs::SHIMS)?;

    let shims: HashSet<String> = ts
        .list_installed_versions(config)?
        .into_par_iter()
        .flat_map(|(t, tv)| match list_tool_bins(config, &t, &tv) {
            Ok(paths) => paths,
            Err(e) => {
                warn!("Error listing bin paths for {}: {:#}", tv, e);
                Vec::new()
            }
        })
        .collect();

    let shims_to_add = shims.difference(&existing_shims);
    let shims_to_remove = existing_shims.difference(&shims);

    for shim in shims_to_add {
        let symlink_path = dirs::SHIMS.join(shim);
        file::make_symlink(&rtx_bin, &symlink_path).map_err(|err| {
            eyre!(
                "Failed to create symlink from {} to {}: {}",
                rtx_bin.display(),
                symlink_path.display(),
                err
            )
        })?;
    }
    for shim in shims_to_remove {
        let symlink_path = dirs::SHIMS.join(shim);
        remove_all(&symlink_path)?;
    }
    for plugin in config.tools.values() {
        match plugin.plugin_path.join("shims").read_dir() {
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
fn list_tool_bins(config: &Config, t: &Tool, tv: &ToolVersion) -> Result<Vec<String>> {
    Ok(t.list_bin_paths(config, tv)?
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
        data_dir = dirs::ROOT.display(),
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
