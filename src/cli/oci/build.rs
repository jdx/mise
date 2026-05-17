use std::path::PathBuf;

use clap::ValueHint;
use eyre::Result;

use crate::cli::oci::common::{perform_build, short_digest};
use crate::config::Settings;
use crate::file::display_path;
use crate::oci::BuildOptions;

/// [experimental] Build an OCI image from the current mise.toml
///
/// Each tool version becomes its own content-addressable OCI layer. Bumping a
/// tool version only invalidates that tool's layer — other tools, the base
/// image, and config are reused unchanged. The output directory conforms to
/// the OCI image-layout spec and can be consumed by `skopeo`, `crane`, or
/// `podman load`.
///
/// Requires `mise settings experimental=true` (or `MISE_EXPERIMENTAL=1`).
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Build {
    /// Output directory for the OCI image layout
    #[clap(long, short, default_value = "./mise-oci", value_hint = ValueHint::DirPath)]
    output: PathBuf,

    /// Base image reference (overrides [oci].from and the oci.default_from setting)
    #[clap(long)]
    from: Option<String>,

    /// Also include tools from the global / system config (default: project-only)
    ///
    /// By default `mise oci build` only packages tools declared in the
    /// project's mise config (and any parent configs at-or-below the
    /// project root, e.g. a monorepo root config). Personal dev tools in
    /// `~/.config/mise/config.toml` are excluded so they don't bake into a
    /// project image. Pass `--include-global` to revert to the old
    /// "merge all loaded configs" behavior.
    #[clap(long)]
    include_global: bool,

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
        Settings::get().ensure_experimental("mise oci build")?;

        let opts = BuildOptions {
            out_dir: self.output.clone(),
            from: self.from.clone(),
            tag: self.tag.clone(),
            mount_point: self.mount_point.clone(),
            include_mise: !self.no_mise,
        };
        let out = perform_build(opts, self.include_global).await?;

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

    - The image only contains tools from the project's mise config (and
      any configs at-or-below the project root). Tools from
      `~/.config/mise/config.toml` are not included; pass --include-global
      to package them too.
    - asdf and vfox plugins are not supported in v1; use a different backend
      (core, aqua, ubi, github, cargo, npm, go, pipx, spm, http) for each tool.
    - The host mise binary is embedded at /usr/local/bin/mise by default;
      build on the same OS/arch as your target image (or pass --no-mise).
"#
);
