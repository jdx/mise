use std::path::PathBuf;

use eyre::Result;

use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{ConfigPathOptions, resolve_target_config_path};
use crate::file::display_path;
use crate::system::packages::brew::default_tap_url;

/// Add a Homebrew tap URL to [bootstrap.brew.taps]
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemBrewTap {
    /// Tap name, e.g. `owner/repo`
    tap: String,

    /// GitHub URL for the tap. Defaults to https://github.com/<owner>/homebrew-<repo>.git
    #[clap(value_hint = clap::ValueHint::Url)]
    url: Option<String>,

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

impl SystemBrewTap {
    pub fn run(self) -> Result<()> {
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages brew tap railwaycat/emacsmacport</bold>
    $ <bold>mise bootstrap packages brew tap acme/tools https://github.com/acme/homebrew-tools.git</bold>
"#
);
