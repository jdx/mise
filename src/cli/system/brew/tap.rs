use std::path::PathBuf;

use eyre::Result;

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{ConfigPathOptions, resolve_target_config_path};
use crate::file::display_path;
use crate::system::packages::brew::default_tap_url;

/// Add a Homebrew tap URL to the global `[bootstrap.brew.taps]`
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        "mise bootstrap packages brew tap acme/tools https://github.com/acme/brew-tools.git",
        help = "Record a tap whose repository is not acme/homebrew-tools"
    ),
    example(
        "mise bootstrap packages brew tap acme/tools https://github.com/acme/brew-tools.git --local",
        help = "Write the entry to the local config instead"
    )
)]
pub(crate) struct SystemBrewTap {
    /// Tap name, e.g. `owner/repo`
    tap: String,

    /// GitHub URL of the tap; defaults to `https://github.com/<owner>/homebrew-<repo>.git`
    #[usage(value_hint = usage_rs::ValueHint::Url)]
    url: Option<String>,

    /// Write to the local config instead of the global config
    #[usage(long, short)]
    local: bool,

    /// Show the config change without writing it
    #[usage(long, short = 'n')]
    pub(super) dry_run: bool,

    /// Write to this config file or directory
    #[usage(
        long,
        short,
        visible_alias = "file",
        value_name = "PATH",
        conflicts = "local"
    )]
    path: Option<PathBuf>,
}

impl SystemBrewTap {
    pub(crate) fn run(self) -> Result<()> {
        let url = match self.url {
            Some(url) => url,
            None => default_tap_url(&self.tap)?,
        };
        let path = resolve_target_config_path(ConfigPathOptions {
            global: !self.local,
            path: self.path,
            env: None,
            cwd: None,
            prefer_toml: true,
            prevent_home_local: true,
            ..Default::default()
        })?;
        if self.dry_run {
            miseprintln!(
                "{}: [bootstrap.brew.taps].\"{}\" = \"{}\"",
                display_path(&path),
                self.tap,
                url
            );
            return Ok(());
        }
        let mut cf = if path.exists() {
            MiseToml::from_file(&path)?
        } else {
            MiseToml::init(&path)
        };
        cf.update_bootstrap_brew_tap(&self.tap, &url)?;
        cf.save()?;
        info!("{}: added brew tap {}", display_path(&path), self.tap);
        Ok(())
    }
}
