use std::path::PathBuf;

use eyre::Result;

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{ConfigPathOptions, resolve_target_config_path};
use crate::file::display_path;

/// Remove Homebrew tap URLs from [bootstrap.brew.taps]
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["remove", "rm"], after_long_help = AFTER_LONG_HELP)]
pub struct SystemBrewUntap {
    /// Tap name(s), e.g. `owner/repo`
    #[clap(required = true)]
    taps: Vec<String>,

    /// Write to the local config instead of the global config
    #[clap(long, short)]
    local: bool,

    /// Print the config change without writing it
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Write to this config file or directory
    #[clap(long, short, value_name = "PATH", conflicts_with = "local")]
    path: Option<PathBuf>,
}

impl SystemBrewUntap {
    pub fn run(self) -> Result<()> {
        let path = resolve_target_config_path(ConfigPathOptions {
            global: !self.local,
            path: self.path,
            env: None,
            cwd: None,
            prefer_toml: true,
            prevent_home_local: true,
        })?;
        if self.dry_run {
            for tap in &self.taps {
                miseprintln!(
                    "{}: remove [bootstrap.brew.taps].\"{}\"",
                    display_path(&path),
                    tap
                );
            }
            return Ok(());
        }
        if !path.exists() {
            info!(
                "{}: no config file found; nothing to remove",
                display_path(&path)
            );
            return Ok(());
        }
        let mut cf = MiseToml::from_file(&path)?;
        for tap in &self.taps {
            cf.remove_bootstrap_brew_tap(tap)?;
        }
        cf.save()?;
        info!(
            "{}: removed brew taps {}",
            display_path(&path),
            self.taps.join(", ")
        );
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages brew untap railwaycat/emacsmacport</bold>
"#
);
