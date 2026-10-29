use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use console::style;
use eyre::{Result, bail, eyre};
use itertools::Itertools;

use crate::args::{BackendArg, ToolArg};
use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::config::project::{nearest_project_tool_requests, tool_request_set};
use crate::config::{Config, Settings};
use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolSource, ToolVersion, Toolset, ToolsetBuilder};
use crate::ui::install_progress::removal_progress;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt::{self, Confirmation};
use crate::{config, dirs, exit, file};

/// Remove installed tool versions
///
/// Removes installed versions but does not edit mise.toml. Use `mise unuse` to
/// remove a tool from mise.toml and uninstall it. A tool name or version prefix,
/// such as `node` or `node@22`, must match exactly one installed version unless
/// you pass `--all`.
///
/// `--project` removes the installed versions that the project's own config files
/// request. Installs are shared by every project, so other projects that use the
/// same versions have to install them again.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise uninstall node@22.0.0"###,
        help = r###"Uninstall a specific version"###
    ),
    example(
        r###"mise uninstall node"###,
        help = r###"Uninstall node when exactly one version is installed"###
    ),
    example(
        r###"mise uninstall --all node"###,
        help = r###"Uninstall every installed version of node"###
    ),
    example(
        r###"mise uninstall --project --dry-run"###,
        help = r###"Preview the versions this project's config requests"###
    )
)]
pub(crate) struct Uninstall {
    /// Tools to remove, such as node@22.0.0
    #[usage(
        value_name = "INSTALLED_TOOL@VERSION",
        required_unless = ["all", "project"]
    )]
    installed_tool: Vec<ToolArg>,

    /// Remove every installed version of the named tools, or of every tool when none is named
    #[usage(long, short)]
    all: bool,

    /// Remove the installed versions the nearest project config requests
    ///
    /// The project is the nearest directory, starting at the current one and
    /// moving up, that has a mise config file. Every config file in that
    /// directory counts, such as `mise.toml`, `mise.local.toml`, `conf.d`
    /// fragments, `.tool-versions` and idiomatic version files. Configs in parent
    /// directories and global or system configs do not. Every version those
    /// files name is removed, including one that a later file overrides. Lists
    /// what it will remove and asks for confirmation unless `--yes` or a dry run
    /// is given. Tools set to a `path:` or `system` version are never removed.
    #[usage(long, conflicts = ["all", "installed_tool"])]
    project: bool,

    /// Show what would change without changing anything
    #[usage(long, short = 'n')]
    dry_run: bool,

    /// Like --dry-run, but exit with code 1 if there are tools to uninstall
    ///
    /// Use it in scripts that check whether tools need uninstalling.
    #[usage(long)]
    dry_run_code: bool,
}

impl Uninstall {
    pub(super) fn is_dry_run(&self) -> bool {
        self.dry_run || self.dry_run_code
    }

    pub(crate) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let mut project_root = None;
        let tool_versions = if self.project {
            let (root, tool_versions) = self.get_project_tool_versions(&config).await?;
            project_root = Some(root);
            tool_versions
        } else if self.installed_tool.is_empty() && self.all {
            self.get_all_tool_versions(&config).await?
        } else {
            self.get_requested_tool_versions(&config).await?
        };
        let tool_versions = tool_versions
            .into_iter()
            // Variants of one version (different install options) are different
            // installations, told apart by where they live.
            .unique_by(|(_, tv)| {
                (
                    tv.request.ba().short.clone(),
                    tv.version.clone(),
                    tv.install_path(),
                )
            })
            .collect::<Vec<_>>();
        // The version as the user wrote it is resolved with no install options, so
        // under the identity layout it names a directory nothing was installed
        // into when the installed copies carry options. Where a copy of the version
        // exists, only existing copies are candidates.
        let present: HashSet<(String, String)> = tool_versions
            .iter()
            .filter(|(_, tv)| file::entry_exists(tv.install_path()))
            .map(|(_, tv)| (tv.ba().short.clone(), tv.version.clone()))
            .collect();
        let tool_versions = tool_versions
            .into_iter()
            .filter(|(_, tv)| {
                file::entry_exists(tv.install_path())
                    || !present.contains(&(tv.ba().short.clone(), tv.version.clone()))
            })
            .collect::<Vec<_>>();
        if !self.all && !self.project && tool_versions.len() > self.installed_tool.len() {
            let variants = tool_versions
                .iter()
                .filter_map(|(_, tv)| {
                    crate::install_layout::resolver::dir_name_of(&tv.install_path())
                })
                .collect_vec();
            if variants.len() == tool_versions.len()
                && tool_versions
                    .iter()
                    .map(|(_, tv)| (tv.ba().short.clone(), tv.version.clone()))
                    .all_equal()
            {
                bail!(
                    "{} installations match, installed with different options: {}\n\
                     use --all to uninstall all of them",
                    variants.len(),
                    variants.join(", ")
                );
            }
            bail!("multiple tools specified, use --all to uninstall all versions");
        }
        let removed_install_paths = tool_versions
            .iter()
            .map(|(_, tv)| tv.install_path())
            .collect::<Vec<_>>();

        let mpr = MultiProgressReport::get();
        let mut to_remove = Vec::with_capacity(tool_versions.len());
        for (plugin, tv) in tool_versions {
            // `is_version_installed` resolves the install path, so it says no for a link whose
            // target is gone -- and that entry is precisely what someone running `uninstall` is
            // trying to get rid of. Refusing it left the name occupied with no command able to
            // free it. It is still not *installed*: this only decides whether there is something
            // here to remove.
            if !plugin.is_version_installed(&config, &tv, true)
                && !file::entry_exists(tv.install_path())
            {
                warn!("{} is not installed", tv.style());
                continue;
            }
            // `--all` lists what is installed, but a configured request (`27`) can stand
            // for an installation under another name (`temurin-27.0.0+35`) and so name
            // a directory that does not exist. Only stale cache under that name would be
            // reported, as if a second installation were being removed.
            if self.all && !file::entry_exists(tv.install_path()) {
                debug!("{} has no installation to remove", tv.style());
                continue;
            }
            to_remove.push((plugin, tv));
        }
        let has_work = !to_remove.is_empty();
        if let Some(root) = &project_root {
            if !has_work {
                info!(
                    "no installed tools are requested by the config in {}",
                    display_path(root)
                );
            } else if !self.confirm_project(root, &to_remove)? {
                return Ok(());
            }
        }

        // A dry run keeps its per-line report: it exists to be read.
        let mut progress = if self.is_dry_run() {
            None
        } else {
            removal_progress(
                &mpr,
                to_remove
                    .iter()
                    .map(|(_, tv)| (crate::cli::prune::removal_key(tv), tv.style())),
            )
        };
        for (plugin, tv) in to_remove {
            let key = crate::cli::prune::removal_key(&tv);
            let tool = progress
                .as_ref()
                .and_then(|progress| progress.start_tool(&key));
            let pr = match &tool {
                Some(tool) => tool.reporter(),
                None => mpr.add(&tv.style()),
            };
            let result = plugin
                .uninstall_version(&config, &tv, pr.as_ref(), self.is_dry_run())
                .await;
            if let Some(tool) = &tool {
                tool.complete(result.as_ref().err().map(|e| e.to_string()).as_deref());
            }
            if let Err(err) = result {
                error!("{err}");
                return Err(eyre!(err).wrap_err(format!("failed to uninstall {tv}")));
            }
            if self.is_dry_run() {
                pr.finish_with_message("uninstalled (dry-run)".into());
            } else {
                // Removed on purpose: the requests it was selected for choose again.
                crate::install_layout::resolver::forget_selections_of(&tv.install_path());
                if let Err(err) = crate::tool_purgatory::forget_path(&tv.install_path()) {
                    warn!("failed to clear tool purgatory entry: {err:#}");
                }
                if tool.is_none() {
                    pr.finish_with_message("uninstalled".into());
                }
            }
        }
        if let Some(progress) = progress.as_mut() {
            progress.finish(vec![]);
        }

        if self.is_dry_run() {
            if self.dry_run_code && has_work {
                return Err(exit::request(1));
            }
            return Ok(());
        }

        file::touch_dir(&dirs::DATA)?;
        let config = Config::reset().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks_after_removal(
            &config,
            ts,
            &removed_install_paths,
        )
        .await?;

        Ok(())
    }

    /// The installed versions requested by the config files of the nearest
    /// project, and that project's directory.
    async fn get_project_tool_versions(
        &self,
        config: &Arc<Config>,
    ) -> Result<(PathBuf, Vec<(Arc<dyn Backend>, ToolVersion)>)> {
        let (root, requested) = nearest_project_tool_requests(config)?;
        // A path or system request names a directory or program mise did not
        // install, and uninstalling one would delete it.
        let requests = tool_request_set(
            requested
                .into_iter()
                .filter(|tr| !matches!(tr, ToolRequest::Path { .. } | ToolRequest::System { .. })),
        );
        let mut ts = Toolset::from(requests);
        ts.resolve(config).await?;
        Ok((root, ts.list_current_installed_versions(config)))
    }

    /// Say what `--project` selected and, unless this is a dry run, ask before
    /// anything is removed. Returns whether to go ahead.
    fn confirm_project(
        &self,
        root: &PathBuf,
        to_remove: &[(Arc<dyn Backend>, ToolVersion)],
    ) -> Result<bool> {
        for (_, tv) in to_remove {
            info!("{} is requested by {}", tv.style(), tv.request.source());
        }
        if self.is_dry_run() || Settings::get().yes {
            return Ok(true);
        }
        let message = format!(
            "remove {} installed version{} requested by the config in {} ?",
            to_remove.len(),
            if to_remove.len() == 1 { "" } else { "s" },
            display_path(root)
        );
        match prompt::confirm_with_all(message)? {
            Confirmation::Yes => Ok(true),
            Confirmation::No | Confirmation::Unanswered => Ok(false),
            Confirmation::Unavailable => bail!(
                "mise uninstall --project requires confirmation but there was nobody to ask; pass --yes to uninstall non-interactively"
            ),
        }
    }

    async fn get_all_tool_versions(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        let ts = ToolsetBuilder::new().build(config).await?;
        let tool_versions = ts
            .list_installed_versions(config)
            .await?
            .into_iter()
            .collect::<Vec<_>>();
        Ok(tool_versions)
    }
    async fn get_requested_tool_versions(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        let runtimes = ToolArg::double_tool_condition(&self.installed_tool)?;
        let mut tool_versions = Vec::new();
        for ta in runtimes {
            let backend = ta.ba.backend()?;
            let query = ta.tvr.as_ref().map(|tvr| tvr.version()).unwrap_or_default();
            let installed_versions = backend.list_installed_versions();
            let exact_match = installed_versions.iter().find(|v| v == &&query);
            let matches = match exact_match {
                Some(m) => vec![m],
                None => installed_versions
                    .iter()
                    .filter(|v| {
                        installed_version_matches(backend.ba(), &query, v, |candidate| {
                            !backend
                                .fuzzy_match_filter(vec![candidate.to_string()], &query, false)
                                .is_empty()
                        })
                    })
                    .collect_vec(),
            };

            let mut tvs = Vec::new();

            if let Some(tvr) = &ta.tvr {
                let resolved = tvr.resolve(config, &Default::default()).await?;
                // A graph install is the one installation the request names; the
                // resolved version would only count it a second time.
                if !graph_install_stands_for(backend.ba(), &resolved.version, &matches) {
                    tvs.push((backend.clone(), resolved));
                }
            }

            // Installations of a matching version that differ in install options
            // share its name, so they are listed from their receipts.
            for (name, dir) in crate::install_layout::resolver::installs_of(backend.ba()) {
                if matches.iter().any(|m| **m == name) {
                    let tvr = ToolRequest::new(backend.ba().clone(), &name, ToolSource::Unknown)?;
                    let version = tvr.version();
                    let mut tv = ToolVersion::new(tvr, version);
                    tv.install_path = Some(dir);
                    tvs.push((backend.clone(), tv));
                }
            }
            tvs.extend(
                matches
                    .into_iter()
                    .map(|v| {
                        let tvr = ToolRequest::new(backend.ba().clone(), v, ToolSource::Unknown)?;
                        // `request.version()`, not the raw name: a ref install is named
                        // `ref-main` on disk and `ref:main` everywhere else.
                        let version = tvr.version();
                        let mut tv = ToolVersion::new(tvr, version);
                        // The name came from a version link or a receipt, so it names an
                        // exact installation, whatever options it was installed with.
                        tv.install_path =
                            crate::install_layout::resolver::physical_dir(backend.ba(), v);
                        Ok((backend.clone(), tv))
                    })
                    .collect::<Result<Vec<_>>>()?,
            );

            if tvs.is_empty() {
                warn!(
                    "no versions found for {}",
                    style(&backend).blue().for_stderr()
                );
            }
            tool_versions.extend(tvs);
        }
        Ok(tool_versions)
    }
}

/// The version a graph install of `ba` stands for: `1.0.0` for
/// `1.0.0~aube~<digest>` of an npm tool or `1.0.0~uv~<digest>` of a pipx tool.
/// Another backend's version may contain these characters, so they say nothing
/// about it.
fn graph_install_version<'a>(ba: &BackendArg, name: &'a str) -> Option<&'a str> {
    let marker = if ba.full_without_opts().starts_with("npm:") {
        "~aube~"
    } else if ba.backend_type() == BackendType::Pipx {
        "~uv~"
    } else {
        return None;
    };
    let (version, identity) = name.rsplit_once(marker)?;
    (identity.len() == 16 && identity.bytes().all(|b| b.is_ascii_hexdigit())).then_some(version)
}

/// Whether `matches` holds a graph install of `resolved` and no plain install
/// of it.
fn graph_install_stands_for(ba: &BackendArg, resolved: &str, matches: &[&String]) -> bool {
    !matches.iter().any(|m| m.as_str() == resolved)
        && matches
            .iter()
            .any(|m| graph_install_version(ba, m) == Some(resolved))
}

/// Whether the installed version `name` is selected by the version `query`,
/// where `matches` is the backend's own fuzzy matcher (Java, for one, accepts
/// `+` after any query).
///
/// `latest` is resolved separately; as a fuzzy query it would select every
/// installed version. A graph install lists as `<version>~aube~<digest>` or
/// `<version>~uv~<digest>`, and the query names the version it stands for.
fn installed_version_matches(
    ba: &BackendArg,
    query: &str,
    name: &str,
    matches: impl Fn(&str) -> bool,
) -> bool {
    if query.is_empty() {
        return true;
    }
    if query == "latest" {
        return false;
    }
    let logical = graph_install_version(ba, name);
    matches(name) || logical.is_some_and(matches)
}

#[cfg(test)]
mod tests {
    use super::{graph_install_stands_for, installed_version_matches};
    use crate::args::BackendArg;

    /// Stands in for the backend's matcher: the query, or the query followed
    /// by a separator and more.
    fn separator_match(query: &str) -> impl Fn(&str) -> bool + '_ {
        move |candidate| {
            candidate == query
                || candidate
                    .strip_prefix(query)
                    .is_some_and(|rest| rest.len() > 1 && rest.starts_with(['.', '-', '+']))
        }
    }

    fn matches(ba: &str, query: &str, name: &str) -> bool {
        installed_version_matches(&BackendArg::from(ba), query, name, separator_match(query))
    }

    #[test]
    fn query_matches_on_a_separator_boundary() {
        assert!(matches("tiny", "3.4.1", "3.4.1"));
        assert!(matches("tiny", "3.4", "3.4.12"));
        assert!(matches("tiny", "3.4.1", "3.4.1-rc1"));
        assert!(!matches("tiny", "3.4.1", "3.4.12"));
        assert!(matches("tiny", "", "3.4.12"));
        assert!(!matches("tiny", "latest", "3.4.12"));
    }

    #[test]
    fn query_matches_the_version_behind_a_graph_identity() {
        let uv = "1.0.0~uv~0123456789abcdef";
        let aube = "1.0.0~aube~0123456789abcdef";
        assert!(matches("pipx:foo", "1.0.0", uv));
        assert!(matches("pipx:foo", "1", uv));
        assert!(matches("npm:foo", "1.0.0", aube));
        assert!(!matches("pipx:foo", "1.0.0", "1.0.1~uv~0123456789abcdef"));
        assert!(!matches("pipx:foo", "1.0.0", "1.0.0~uv~nothex"));
    }

    #[test]
    fn a_suffix_is_a_graph_identity_only_for_its_backend() {
        let uv = "1.0.0~uv~0123456789abcdef";
        let aube = "1.0.0~aube~0123456789abcdef";
        assert!(!matches("npm:foo", "1.0.0", uv));
        assert!(!matches("pipx:foo", "1.0.0", aube));
        assert!(!matches("tiny", "1.0.0", uv));
        assert!(!matches("tiny", "1.0.0", aube));
    }

    #[test]
    fn a_graph_install_stands_for_the_version_it_was_resolved_from() {
        let pipx = BackendArg::from("pipx:foo");
        let graph = "1.0.0~uv~0123456789abcdef".to_string();
        let other = "1.0.1".to_string();
        let plain = "1.0.0".to_string();
        assert!(graph_install_stands_for(&pipx, "1.0.0", &[&graph]));
        assert!(graph_install_stands_for(&pipx, "1.0.0", &[&graph, &other]));
        assert!(!graph_install_stands_for(&pipx, "1.0.0", &[&graph, &plain]));
        assert!(!graph_install_stands_for(&pipx, "1.0.1", &[&graph]));
        assert!(!graph_install_stands_for(&pipx, "1.0.0", &[]));
        // Another backend's version name is its own.
        assert!(!graph_install_stands_for(
            &BackendArg::from("tiny"),
            "1.0.0",
            &[&graph]
        ));
    }
}
