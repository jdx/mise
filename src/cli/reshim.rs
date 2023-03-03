use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::env::RTX_EXE;
use crate::file::{create_dir_all, remove_dir_all};
use crate::output::Output;
use crate::toolset::ToolsetBuilder;
use crate::{dirs, fake_asdf};

/// [experimental] rebuilds the shim farm
///
/// this requires that the shims_dir is set
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Reshim {
    #[clap(hide = true)]
    pub plugin: Option<String>,
    #[clap(hide = true)]
    pub version: Option<String>,
}

impl Command for Reshim {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&config);

        if !config.settings.experimental {
            err_experimental()?;
        }
        let shims_dir = get_shims_dir(&config)?;

        // remove old shims
        let _ = remove_dir_all(&shims_dir);
        create_dir_all(&shims_dir)?;

        for path in ts.list_paths(&config.settings) {
            if !path.exists() {
                continue;
            }
            for bin in path.read_dir()? {
                let bin = bin?;
                if !bin.file_type()?.is_file() && !bin.file_type()?.is_symlink() {
                    continue;
                }
                let bin_name = bin.file_name().into_string().unwrap();
                let symlink_path = shims_dir.join(bin_name);
                make_symlink(&RTX_EXE, &symlink_path)?;
            }
        }
        for plugin in config.plugins.values() {
            match plugin.plugin_path.join("shims").read_dir() {
                Ok(files) => {
                    for bin in files {
                        let bin = bin?;
                        let bin_name = bin.file_name().into_string().unwrap();
                        let symlink_path = shims_dir.join(bin_name);
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
}

fn make_symlink(target: &Path, link: &Path) -> Result<()> {
    if link.exists() {
        fs::remove_file(link)?;
    }
    symlink(target, link)?;
    debug!("symlinked {} to {}", target.display(), link.display());
    Ok(())
}

fn make_shim(target: &Path, shim: &Path) -> Result<()> {
    if shim.exists() {
        fs::remove_file(shim)?;
    }
    fs::write(
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
    let mut perms = shim.metadata()?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(shim, perms)?;
    debug!(
        "shim created from {} to {}",
        target.display(),
        shim.display()
    );
    Ok(())
}

fn get_shims_dir(config: &Config) -> Result<PathBuf> {
    match config.settings.shims_dir.clone() {
        Some(mut shims_dir) => {
            if shims_dir.starts_with("~") {
                shims_dir = dirs::HOME.join(shims_dir.strip_prefix("~")?);
            }
            Ok(shims_dir)
        }
        None => err_no_shims_dir(),
    }
}

fn err_experimental() -> Result<()> {
    return Err(eyre!(formatdoc!(
        r#"
                rtx is not configured to use experimental features.
                Please set the `{}` setting to `true`.
                "#,
        style("experimental").yellow()
    )));
}

fn err_no_shims_dir() -> Result<PathBuf> {
    return Err(eyre!(formatdoc!(
        r#"
                rtx is not configured to use shims.
                Please set the `{}` setting to a directory.
                "#,
        style("shims_dir").yellow()
    )));
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx settings set experimental true
      $ rtx settings set shims_dir ~/.rtx/shims
      $ rtx reshim
      $ ~/.rtx/shims/node -v
      v18.0.0
    "#, style("Examples:").bold().underlined()}
});
