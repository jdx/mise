use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::Arc;

use itertools::Itertools;
use miette::{IntoDiagnostic, Result, WrapErr};
use rayon::prelude::*;

use crate::cli::exec::Exec;
use crate::config::{Config, Settings};
use crate::fake_asdf;
use crate::file::{create_dir_all, display_path, remove_all};
use crate::lock_file::LockFile;
use crate::{env, logger};

use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::{dirs, file};

// executes as if it was a shim if the command is not "mise", e.g.: "node"
pub fn handle_shim() -> Result<()> {
    // TODO: instead, check if bin is in shims dir
    let bin_name = *env::MISE_BIN_NAME;
    if bin_name == "mise" || !dirs::SHIMS.join(bin_name).exists() || cfg!(test) {
        return Ok(());
    }
    logger::init(&Settings::get());
    let args = env::ARGS.read().unwrap();
    let mut args: Vec<OsString> = args.iter().map(OsString::from).collect();
    args[0] = which_shim(&env::MISE_BIN_NAME)?.into();
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
    let config = Config::try_get()?;
    let mut ts = ToolsetBuilder::new().build(&config)?;
    if let Some((p, tv)) = ts.which(bin_name) {
        if let Some(bin) = p.which(&tv, bin_name)? {
            return Ok(bin);
        }
    }
    let settings = Settings::try_get()?;
    if settings.not_found_auto_install {
        for tv in ts.install_missing_bin(bin_name)?.unwrap_or_default() {
            let p = config.get_or_create_plugin(&tv.plugin_name);
            if let Some(bin) = p.which(&tv, bin_name)? {
                return Ok(bin);
            }
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
    let tvs = ts.list_rtvs_with_bin(&config, bin_name)?;
    err_no_version_set(ts, bin_name, tvs)
}

pub fn reshim(config: &Config, ts: &Toolset) -> Result<()> {
    let _lock = LockFile::new(&dirs::SHIMS)
        .with_callback(|l| {
            trace!("reshim callback {}", l.display());
        })
        .lock();

    let mise_bin = file::which("mise").unwrap_or(env::MISE_BIN.clone());

    create_dir_all(&*dirs::SHIMS)?;

    let existing_shims = list_executables_in_dir(&dirs::SHIMS)?
        .into_par_iter()
        .filter(|bin| {
            dirs::SHIMS
                .join(bin)
                .read_link()
                .is_ok_and(|p| p == mise_bin)
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
        file::make_symlink(&mise_bin, &symlink_path).wrap_err_with(|| {
            miette!(
                "Failed to create symlink from {} to {}",
                display_path(&mise_bin),
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
                    let bin = bin.into_diagnostic()?;
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
    for bin in dir.read_dir().into_diagnostic()? {
        let bin = bin.into_diagnostic()?;
        // skip non-files and non-symlinks or non-executable files
        if (!bin.file_type().into_diagnostic()?.is_file()
            && !bin.file_type().into_diagnostic()?.is_symlink())
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
    let missing_plugins = tvs.iter().map(|tv| &tv.plugin_name).collect::<HashSet<_>>();
    let mut missing_tools = ts
        .list_missing_versions()
        .into_iter()
        .filter(|t| missing_plugins.contains(&t.plugin_name))
        .collect_vec();
    if missing_tools.is_empty() {
        let mut msg = format!("No version is set for shim: {}\n", bin_name);
        msg.push_str("Set a global default version with one of the following:\n");
        for tv in tvs {
            msg.push_str(&format!("mise use -g {}@{}\n", tv.plugin_name, tv.version));
        }
        Err(miette!(msg.trim().to_string()))
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
        Err(miette!(msg.trim().to_string()))
    }
}
