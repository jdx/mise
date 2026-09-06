use std::{path::PathBuf, sync::Arc};

use crate::cli::args::ToolArg;
use crate::cli::prune::prune;
use crate::config::config_file::ConfigFile;
use crate::config::{Config, config_file};
use crate::file::display_path;
use crate::{config, env, shims};
use eyre::Result;
use itertools::Itertools;
use path_absolutize::Absolutize;

/// Remove tool requests from configuration and prune unused installations
///
/// Without a selector, mise edits the first loaded config that declares one of the
/// requested tools. Use `--path`, `--global`, or `--env` to choose a specific file.
/// A version argument matches the configured request literally: to remove `node = "20"`,
/// use `mise unuse node@20`, not the concrete installed version it resolved to.
/// Omit the version to remove all requests for that tool from the selected file.
///
/// Versions are pruned only when no remaining tracked config or tool stub needs them.
/// Pass `--no-prune` to edit configuration while keeping installations. To remove an
/// installation without editing configuration, use `mise uninstall`.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, visible_aliases = ["rm", "remove"], example(r###"mise unuse node@18.0.0"###, help = r###"remove node@18.0.0 from mise.toml and uninstall it"###),
    example(r###"mise unuse -g node@18.0.0"###, help = r###"remove it from the global config instead"###),
    example(r###"mise unuse --env local node@20"###, help = r###"remove the literal node@20 request from mise.local.toml"###),
    example(r###"mise unuse --env staging node@20"###, help = r###"remove the literal node@20 request from mise.staging.toml"###))]
pub(crate) struct Unuse {
    /// Tool(s) to remove
    #[usage(value_name = "INSTALLED_TOOL@VERSION", required = true)]
    installed_tool: Vec<ToolArg>,

    /// Create/modify an environment-specific config file like mise.<env>.toml
    #[usage(long, short, overrides = & ["global", "path"])]
    env: Option<String>,

    /// Use the global config file (`~/.config/mise/config.toml`) instead of the local one
    #[usage(short, long, overrides = & ["path", "env"])]
    global: bool,

    /// Specify a path to a config file or directory
    ///
    /// If a directory is specified, it will look for a config file in that directory following
    /// the target-file selection rules.
    #[usage(short, long, visible_alias = "file", overrides = & ["global", "env"], value_hint = usage_rs::ValueHint::FilePath)]
    path: Option<PathBuf>,

    /// Do not also prune the installed version
    #[usage(long)]
    no_prune: bool,
}

impl Unuse {
    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let cf = self.get_config_file(&config).await?;
        let system_config = config::is_system_config(cf.get_path());
        let tools = cf.to_tool_request_set()?.tools;
        let mut removed: Vec<&ToolArg> = vec![];
        for ta in &self.installed_tool {
            let already_removed = removed.iter().any(|existing| {
                existing.ba.as_ref() == ta.ba.as_ref() && existing.version == ta.version
            });
            if already_removed {
                continue;
            }
            let Some(tool_requests) = tools.get(ta.ba.as_ref()) else {
                continue;
            };
            let matches = match &ta.version {
                Some(version) => tool_requests.iter().any(|tr| tr.version() == *version),
                None => true,
            };
            if matches {
                removed.push(ta);
            }
        }

        for (ba, tool_requests) in &tools {
            let matching_args = removed
                .iter()
                .copied()
                .filter(|ta| ta.ba.as_ref() == ba.as_ref())
                .collect_vec();
            if matching_args.is_empty() {
                continue;
            }

            if matching_args.iter().any(|ta| ta.version.is_none()) {
                cf.remove_tool(ba)?;
                continue;
            }

            let remaining = tool_requests
                .iter()
                .filter(|tr| {
                    let version = tr.version();
                    !matching_args
                        .iter()
                        .any(|ta| ta.version.as_deref() == Some(version.as_str()))
                })
                .cloned()
                .collect_vec();
            if remaining.len() == tool_requests.len() {
                continue;
            }
            if remaining.is_empty() {
                cf.remove_tool(ba)?;
            } else {
                cf.replace_versions(ba, remaining)?;
            }
        }
        if removed.is_empty() {
            debug!("no tools to remove");
        } else {
            cf.save()?;
            let removals = removed.iter().join(", ");
            info!("removed: {removals} from {}", display_path(cf.get_path()));
        }

        if !self.no_prune {
            prune(
                &config,
                self.installed_tool
                    .iter()
                    .map(|ta| ta.ba.as_ref())
                    .collect(),
                false,
            )
            .await?;
        }
        if !removed.is_empty() || !self.no_prune {
            let shim_scope = match (system_config, self.no_prune) {
                (true, false) => shims::ShimScope::Both,
                (true, true) => shims::ShimScope::System,
                (false, _) => shims::ShimScope::User,
            };
            let config = Config::reset().await?;
            let ts = config.get_toolset().await?;
            config::rebuild_shims_and_runtime_symlinks_for_scope(&config, ts, shim_scope).await?;
        }

        Ok(())
    }

    async fn get_config_file(&self, config: &Config) -> Result<Arc<dyn ConfigFile>> {
        let cwd = env::current_dir()?;
        let path = if self.global {
            config::global_config_path()
        } else if let Some(p) = &self.path {
            let p = p.absolutize()?.to_path_buf();
            if p.is_dir() {
                config::config_file_in_dir(&p)
            } else {
                p
            }
        } else if let Some(env) = &self.env {
            let p = cwd.join(format!(".mise.{env}.toml"));
            if p.exists() {
                p
            } else {
                cwd.join(format!("mise.{env}.toml"))
            }
        } else if env::in_home_dir() {
            config::global_config_path()
        } else {
            for cf in config.config_files.values() {
                if cf
                    .to_tool_request_set()?
                    .tools
                    .keys()
                    .any(|ba| self.installed_tool.iter().any(|ta| &ta.ba == ba))
                {
                    return config_file::parse(cf.get_path()).await;
                }
            }
            config::local_toml_config_path()
        };
        config_file::parse_or_init(&path).await
    }
}
