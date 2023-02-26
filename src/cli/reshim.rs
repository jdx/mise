use std::fs::create_dir_all;
use std::os::unix::fs;
use std::path::PathBuf;

use color_eyre::eyre::{eyre, Result};
use console::style;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::dirs;
use crate::env::RTX_EXE;
use crate::output::Output;
use crate::toolset::ToolsetBuilder;

/// [experimental] rebuilds the shim farm
///
/// this requires that the shim_dir is set
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
pub struct Reshim {
    #[clap(hide = true)]
    pub plugin: Option<String>,
}

impl Command for Reshim {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().with_install_missing().build(&config);

        if !config.settings.experimental {
            err_experimental()?;
        }
        let shims_dir = get_shims_dir(&config)?;

        for path in ts.list_paths() {
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
                if !symlink_path.exists() {
                    fs::symlink(&*RTX_EXE, &symlink_path)?;
                    debug!(
                        "symlinked {} to {}",
                        bin.path().display(),
                        symlink_path.display()
                    );
                }
            }
        }

        Ok(())
    }
}

fn get_shims_dir(config: &Config) -> Result<PathBuf> {
    match config.settings.shims_dir.clone() {
        Some(mut shims_dir) => {
            if shims_dir.starts_with("~") {
                shims_dir = dirs::HOME.join(shims_dir.strip_prefix("~")?);
            }
            create_dir_all(&shims_dir)?;
            Ok(shims_dir)
        }
        None => err_no_shim_dir(),
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

fn err_no_shim_dir() -> Result<PathBuf> {
    return Err(eyre!(formatdoc!(
        r#"
                rtx is not configured to use shims.
                Please set the `{}` setting to a directory.
                "#,
        style("shim_dir").yellow()
    )));
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx settings set experimental true
      $ rtx settings set shim_dir ~/.rtx/shims
      $ rtx reshim
      $ ~/.rtx/shims/node -v
      v20.0.0
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {

    // #[test]
    // fn test_reshim() {
    //     assert_cli!("local", "dummy@1.0.0");
    //     assert_cli_snapshot!("reshim");
    //     assert_cli_snapshot!("x", "--", "ls", "../data/shims");
    //     assert_cli!("uninstall", "dummy@1.0.0");
    //     assert_cli!("local", "--rm", "dummy@1.0.0");
    // }
}
