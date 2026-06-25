use std::path::PathBuf;

use eyre::Result;
use indexmap::IndexMap;

use super::driver::{self, Action, DriverOpts};
use crate::config::config_file::ConfigFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::{ConfigPathOptions, Settings, resolve_target_config_path};
use crate::file::display_path;
use crate::system;
use crate::system::packages::PackageRequest;

/// Add bootstrap packages to [bootstrap.packages] and install them
///
/// Like `mise use` for tools: writes `"manager:package" = "version"` entries
/// to mise.toml (the local config by default, the global one with `-g`) and
/// then installs whatever is missing.
///
/// Versions are pinned with `@`: `mise bootstrap packages use apt:curl@8.5.0-2`. Without
/// `@` (or with `@latest`) no pin is written. brew formulae and casks
/// version through their names instead (for example `brew:postgresql@17`,
/// `brew-cask:temurin@17`), where `@` is part of the Homebrew name rather than
/// a mise version selector. mas uses numeric ADAM IDs and does not support pins.
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "u", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct SystemUse {
    /// Packages in `manager:package[@version]` form
    #[clap(value_name = "PACKAGE", required = true)]
    packages: Vec<String>,

    /// Write to the config file for this environment (mise.<ENV>.toml)
    #[clap(long, short, value_name = "ENV", conflicts_with_all = ["global", "path"])]
    env: Option<String>,

    /// Write to the global config (~/.config/mise/config.toml) instead of the
    /// local one
    #[clap(long, short)]
    global: bool,

    /// Print the commands that would run without writing config or installing
    #[clap(long, short = 'n')]
    dry_run: bool,

    /// Write to this config file or directory
    #[clap(long, short, value_name = "PATH", conflicts_with = "global")]
    path: Option<PathBuf>,

    /// Skip the confirmation prompt
    #[clap(long, short)]
    yes: bool,
}

impl SystemUse {
    pub async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise bootstrap")?;
        let config = crate::config::Config::get().await?;
        let mut by_mgr: IndexMap<String, Vec<PackageRequest>> = IndexMap::new();
        let mut entries: Vec<(String, String)> = vec![];
        for spec in &self.packages {
            let (mgr, request) = system::parse_use_spec(spec)?;
            let key = format!("{mgr}:{}", request.name);
            let version = request.version.clone().unwrap_or_else(|| "latest".into());
            // the same package twice: the last version wins, in the config
            // entry and the install request alike
            match entries.iter_mut().find(|(k, _)| k == &key) {
                Some(entry) => entry.1 = version,
                None => entries.push((key, version)),
            }
            let requests = by_mgr.entry(mgr).or_default();
            match requests.iter_mut().find(|r| r.name == request.name) {
                Some(r) => *r = request,
                None => requests.push(request),
            }
        }
        system::attach_brew_tap_urls(&config, &mut by_mgr);
        // resolve managers before touching the config file so a typo'd
        // manager doesn't get written
        let mgrs = system::packages_from_requests(by_mgr)?;

        let path = resolve_target_config_path(ConfigPathOptions {
            global: self.global,
            path: self.path.clone(),
            env: self.env.clone(),
            cwd: None,
            prefer_toml: true,        // [bootstrap] only exists in mise.toml
            prevent_home_local: true, // in $HOME, write the global config
        })?;
        if self.dry_run {
            for (key, version) in &entries {
                miseprintln!("{}: \"{key}\" = \"{version}\"", display_path(&path));
            }
        } else {
            let mut cf = if path.exists() {
                MiseToml::from_file(&path)?
            } else {
                MiseToml::init(&path)
            };
            for (key, version) in &entries {
                cf.update_bootstrap_package(key, version)?;
            }
            cf.save()?;
            info!(
                "{}: added {}",
                display_path(&path),
                entries
                    .iter()
                    .map(|(k, _)| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        // unlike `mise bootstrap packages apply apt:x`, an unavailable manager is not
        // an error here: writing apt: entries from a mac into a shared repo
        // config is the point of a declarative file. Say so (except in
        // dry-run, where nothing was written), then install best-effort for
        // this machine.
        if !self.dry_run {
            for mp in &mgrs {
                if !mp.disabled && !mp.manager.is_available() {
                    info!(
                        "{}: {} — added to config without installing",
                        mp.manager.name(),
                        mp.manager.unavailable_reason()
                    );
                }
            }
        }
        let opts = DriverOpts {
            manager: None,
            explicit: false,
            dry_run: self.dry_run,
            update: false,
            yes: self.yes,
        };
        driver::run(mgrs, Action::Install, &opts).await
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise bootstrap packages use apk:zlib-dev apt:curl brew:jq brew-cask:firefox mas:497799835</bold>
    $ <bold>mise bootstrap packages use -g brew:postgresql@17</bold>
    $ <bold>mise bootstrap packages use apt:curl@8.5.0-2</bold>
"#
);
