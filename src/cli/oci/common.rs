//! Shared helpers for `mise oci` subcommands.

use eyre::{Result, bail};

use crate::config::{Config, ConfigMap};
use crate::oci::{BuildOptions, BuildOutput, Builder, OciConfig};
use crate::toolset::{ConfigScope, ToolsetBuilder};

/// Merge `[oci]` sections across all loaded config files, with more specific
/// (closer to the current directory) configs winning per-field.
///
/// Uses "first-Some-wins" semantics via `OciConfig::fill_defaults_from`, so
/// the result is correct as long as mise's `config_files` iterates
/// most-specific-first — the convention at the time of writing. If that
/// convention ever inverts, this logic still picks the first value it sees
/// and just changes which config wins; it won't silently drop fields or
/// return garbage.
pub fn merged_oci_config(config: &Config) -> OciConfig {
    let mut out = OciConfig::default();
    for cf in config.config_files.values() {
        if let Some(oci) = cf.oci_config() {
            out.fill_defaults_from(oci);
        }
    }
    out
}

/// Load config + toolset, merge the `[oci]` section, and build the image.
/// Used by `mise oci build`, `mise oci run`, and `mise oci push`.
///
/// By default the toolset is scoped to configs at-or-below the project root
/// — i.e. the user's global config, system config, and parent-directory
/// configs above the project (e.g. a `~/mise.toml` setting personal Node
/// defaults) are excluded. Rationale: `mise oci build` is conceptually
/// "package *this project's* tools into a deployable image" — personal dev
/// tools (neovim, ripgrep, …) sitting in `~/.config/mise/config.toml` have
/// no business in a project image, and several of them (asdf/vfox plugins)
/// would in fact be rejected by the v1 builder. See discussion #9690.
///
/// Set `include_global = true` to revert to the merge-everything behavior.
pub async fn perform_build(opts: BuildOptions, include_global: bool) -> Result<BuildOutput> {
    let config = Config::get().await?;
    let ts = build_toolset(&config, include_global).await?;
    let oci = merged_oci_config(&config);
    Builder::new(config.clone(), ts, oci, opts).build().await
}

async fn build_toolset(
    config: &std::sync::Arc<Config>,
    include_global: bool,
) -> Result<crate::toolset::Toolset> {
    if include_global {
        return ToolsetBuilder::new().build(config).await;
    }
    // `cf.project_root().is_some()` is the right scope: it's None for the
    // global config, the system config, and parent-dir configs that live
    // directly under $HOME (e.g. `~/mise.toml` someone uses to set their
    // default Node). It's Some for any project config, including those
    // walked up from CWD into a monorepo root — which we *do* want included
    // (sub-project + monorepo root share the toolset of the deployable).
    let project_files: ConfigMap = config
        .config_files
        .iter()
        .filter(|(_, cf)| cf.project_root().is_some())
        .map(|(p, cf)| (p.clone(), cf.clone()))
        .collect();
    if project_files.is_empty() {
        bail!(
            "mise oci build: no project mise config found in the current directory or any \
             parent. Add a `mise.toml` to the project, or pass `--include-global` to package \
             tools from your global config (note: asdf/vfox plugins remain unsupported)."
        );
    }
    // ConfigScope::LocalOnly belt-and-suspenders: the project-files filter
    // already drops global/system, but LocalOnly *also* drops MISE_*_VERSION
    // ad-hoc overrides from the environment, which shouldn't bake into an
    // image either.
    ToolsetBuilder::new()
        .with_config_files(project_files)
        .with_scope(ConfigScope::LocalOnly)
        .build(config)
        .await
}

pub fn short_digest(d: &str) -> String {
    let hex = d.trim_start_matches("sha256:");
    if hex.len() >= 12 {
        format!("sha256:{}", &hex[..12])
    } else {
        d.to_string()
    }
}
