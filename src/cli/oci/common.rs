//! Shared helpers for `mise oci` subcommands.

use eyre::Result;

use crate::config::Config;
use crate::oci::{BuildOptions, BuildOutput, Builder, OciConfig};
use crate::toolset::ToolsetBuilder;

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
