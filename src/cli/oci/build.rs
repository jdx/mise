use std::path::PathBuf;

use clap::ValueHint;
use eyre::Result;

use crate::config::Config;
use crate::file::display_path;
use crate::oci::{BuildOptions, Builder, OciConfig};
use crate::toolset::ToolsetBuilder;

/// Build an OCI image from the current mise.toml
///
/// Each tool version becomes its own content-addressable OCI layer. Bumping a
/// tool version only invalidates that tool's layer — other tools, the base
/// image, and config are reused unchanged. The output directory conforms to
/// the OCI image-layout spec and can be consumed by `skopeo`, `crane`, or
/// `podman load`.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Build {
    /// Output directory for the OCI image layout
    #[clap(long, short, default_value = "./mise-oci", value_hint = ValueHint::DirPath)]
    output: PathBuf,

    /// Base image reference (overrides [oci].from and the oci.default_from setting)
    #[clap(long)]
    from: Option<String>,

    /// Tag to record in the image index (the org.opencontainers.image.ref.name annotation)
    #[clap(long, short)]
    tag: Option<String>,

    /// Where to place tool installs inside the image (default: /mise)
    #[clap(long)]
    mount_point: Option<String>,

    /// Do not embed the currently-running mise binary at /usr/local/bin/mise
    #[clap(long)]
    no_mise: bool,
}

impl Build {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let ts = ToolsetBuilder::new().build(&config).await?;

        let oci = merged_oci_config(&config);

        let opts = BuildOptions {
            out_dir: self.output.clone(),
            from: self.from.clone(),
            tag: self.tag.clone(),
            mount_point: self.mount_point.clone(),
            include_mise: !self.no_mise,
        };

        let builder = Builder::new(config.clone(), ts, oci, opts);
        let out = builder.build().await?;

        miseprintln!("wrote OCI image layout to {}", display_path(&out.out_dir));
        miseprintln!("manifest: {}", out.manifest_digest);
        miseprintln!("tool layers:");
        for l in &out.tool_layers {
            miseprintln!(
                "  {}@{}  {}  {} bytes",
                l.short,
                l.version,
                short_digest(&l.digest),
                l.size
            );
        }
        Ok(())
    }
}

/// Merge `[oci]` sections across all loaded config files, with more specific
/// (closer to the current directory) configs overriding less specific ones.
///
/// `config_files` iterates most-specific-first, so we `.rev()` to start from
/// the least-specific and let each subsequent overlay win.
fn merged_oci_config(config: &crate::config::Config) -> OciConfig {
    config
        .config_files
        .values()
        .rev()
        .filter_map(|cf| cf.oci_config())
        .fold(OciConfig::default(), |acc, cur| acc.overlay(cur))
}

fn short_digest(d: &str) -> String {
    let hex = d.trim_start_matches("sha256:");
    if hex.len() >= 12 {
        format!("sha256:{}", &hex[..12])
    } else {
        d.to_string()
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    Build with defaults (debian:bookworm-slim base):
    $ <bold>mise oci build</bold>

    Build with a specific base image and tag:
    $ <bold>mise oci build --from ubuntu:24.04 --tag myorg/dev:latest -o ./img</bold>

    Inspect the result with skopeo:
    $ <bold>skopeo inspect oci:./mise-oci</bold>

    Push to a registry:
    $ <bold>skopeo copy oci:./mise-oci docker://ghcr.io/me/dev:latest</bold>

<bold><underline>Notes:</underline></bold>

    - asdf and vfox plugins are not supported in v1; use a different backend
      (core, aqua, ubi, github, cargo, npm, go, pipx, spm, http) for each tool.
    - The host mise binary is embedded at /usr/local/bin/mise by default;
      build on the same OS/arch as your target image (or pass --no-mise).
"#
);
