//! Shared helpers for `mise oci` subcommands.

use eyre::Result;

use crate::config::Config;
use crate::oci::{BuildOptions, BuildOutput, Builder, OciConfig};
use crate::toolset::ToolsetBuilder;

/// Merge `[oci]` sections across all loaded config files, with more specific
/// (closer to the current directory) configs overriding less specific ones.
///
/// `config_files` iterates most-specific-first, so we `.rev()` to start from
/// the least-specific and let each subsequent overlay win.
pub fn merged_oci_config(config: &Config) -> OciConfig {
    config
        .config_files
        .values()
        .rev()
        .filter_map(|cf| cf.oci_config())
        .fold(OciConfig::default(), |acc, cur| acc.overlay(cur))
}

/// Load config + toolset, merge the `[oci]` section, and build the image.
/// Used by `mise oci build`, `mise oci run`, and `mise oci push`.
pub async fn perform_build(opts: BuildOptions) -> Result<BuildOutput> {
    let config = Config::get().await?;
    let ts = ToolsetBuilder::new().build(&config).await?;
    let oci = merged_oci_config(&config);
    Builder::new(config.clone(), ts, oci, opts).build().await
}

pub fn short_digest(d: &str) -> String {
    let hex = d.trim_start_matches("sha256:");
    if hex.len() >= 12 {
        format!("sha256:{}", &hex[..12])
    } else {
        d.to_string()
    }
}
