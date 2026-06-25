#[cfg(unix)]
use std::collections::BTreeMap;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use eyre::{Result, bail};

#[cfg(unix)]
use crate::config::Config;
use crate::config::Settings;
#[cfg(unix)]
use crate::config::config_file::ConfigFile;
#[cfg(unix)]
use crate::config::config_file::mise_toml::MiseToml;
#[cfg(unix)]
use crate::config::{ConfigPathOptions, resolve_target_config_path};
#[cfg(unix)]
use crate::file::display_path;
#[cfg(unix)]
use crate::system;
#[cfg(unix)]
use crate::system::packages::SystemPackageManager;
#[cfg(unix)]
use crate::system::packages::brew;

/// Import installed system packages into `[bootstrap.packages]`
///
/// Currently supports Homebrew formulae only. By default, imports linked
/// formulae whose active keg receipt says they were installed on request.
/// Pass `--all` to import every linked formula, including dependencies.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemImport {
    /// Write to the config file for this environment (mise.<ENV>.toml)
    #[clap(long, short, value_name = "ENV", conflicts_with_all = ["global", "path"])]
    env: Option<String>,

    /// Write to the global config (~/.config/mise/config.toml)
    #[clap(long, short, conflicts_with_all = ["env", "path"])]
    global: bool,

    /// Only import packages for this manager. Currently only `brew` is supported.
    #[clap(long, short, default_value = "brew", value_parser = ["brew"])]
    manager: String,

    /// Import every linked formula, including dependencies
    #[clap(long)]
    all: bool,

    /// Print the config change without writing config
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Write to this config file or directory
    #[clap(long, short, value_name = "PATH", conflicts_with = "global")]
    path: Option<PathBuf>,
}

impl SystemImport {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        if Settings::get()
            .system_packages
            .managers
            .as_ref()
            .is_some_and(|enabled| !enabled.contains(&self.manager))
        {
            bail!(
                "manager '{}' is excluded by the system_packages.managers setting",
                self.manager
            );
        }
        self.run_brew().await
    }

    #[cfg(unix)]
    async fn run_brew(self) -> Result<()> {
        debug_assert_eq!(self.manager, "brew");
        let manager = brew::BrewManager::new();
        if !manager.is_available() {
            bail!("brew is not available: {}", manager.unavailable_reason());
        }
        let formulae = brew::linked_formulae(self.all)?;
        if formulae.is_empty() {
            info!("brew: no installed formulae to import");
            return Ok(());
        }

        let path = resolve_target_config_path(ConfigPathOptions {
            global: self.global,
            path: self.path.clone(),
            env: self.env.clone(),
            cwd: None,
            prefer_toml: true,
            prevent_home_local: true,
        })?;

        let configured_taps = configured_brew_taps(&path).await?;
        let target_taps = target_brew_taps(&path)?;
        let mut taps = BTreeMap::new();
        for formula in &formulae {
            if let Some((tap, url)) = formula.tap_entry_with_urls(&configured_taps)? {
                if !target_taps.contains_key(&tap) {
                    taps.insert(tap, url);
                }
            }
        }

        if self.dry_run {
            for (tap, url) in &taps {
                miseprintln!(
                    "{}: [bootstrap.brew.taps].\"{}\" = \"{}\"",
                    display_path(&path),
                    tap,
                    url
                );
            }
            for formula in &formulae {
                miseprintln!(
                    "{}: \"{}\" = \"latest\"",
                    display_path(&path),
                    formula.config_key()
                );
            }
            return Ok(());
        }

        let mut cf = if path.exists() {
            MiseToml::from_file(&path)?
        } else {
            MiseToml::init(&path)
        };
        for (tap, url) in &taps {
            cf.update_bootstrap_brew_tap(tap, url)?;
        }
        for formula in &formulae {
            cf.update_bootstrap_package(&formula.config_key(), "latest")?;
        }
        cf.save()?;
        info!(
            "{}: imported {} brew formulae",
            display_path(&path),
            formulae.len()
        );
        Ok(())
    }

    #[cfg(not(unix))]
    async fn run_brew(self) -> Result<()> {
        let _ = self.manager;
        bail!("brew import is not supported on windows")
    }
}

#[cfg(unix)]
async fn configured_brew_taps(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut taps = BTreeMap::new();
    let config = Config::get().await?;
    for (tap, url) in system::brew_taps_from_config(&config) {
        taps.insert(tap, url);
    }
    for (tap, url) in target_brew_taps(path)? {
        taps.insert(tap, url);
    }
    Ok(taps)
}

#[cfg(unix)]
fn target_brew_taps(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut taps = BTreeMap::new();
    if path.exists() {
        let cf = MiseToml::from_file(path)?;
        if let Some(sys) = cf.bootstrap_config() {
            for (tap, url) in sys.brew.taps {
                taps.insert(tap, url);
            }
        }
    }
    Ok(taps)
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages import --manager brew</bold>
    $ <bold>mise bootstrap packages import --manager brew --all</bold>
    $ <bold>mise bootstrap packages import --manager brew --global</bold>
    $ <bold>mise bootstrap packages import --manager brew --dry-run</bold>
"#
);
