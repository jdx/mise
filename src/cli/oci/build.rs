use std::path::PathBuf;

use eyre::Result;

use crate::cli::oci::common::{mise_binary_path, perform_build, short_digest};
use crate::config::Settings;
use crate::file::display_path;
use crate::oci::{BuildOptions, LayerOwner, OciCopy};

/// [experimental] Build an OCI image from the current mise.toml
///
/// Each tool version becomes its own content-addressable OCI layer. Bumping a
/// tool version invalidates that tool's layer (a Python bump also invalidates
/// pypi tool layers, which are relocated against it); other tool layers and the
/// base image layers are reused, while the generated /etc/mise/config.toml
/// layer, image config, and manifest are regenerated. The output directory
/// follows the OCI image-layout spec.
///
/// Only tools from project config files are packaged, including a monorepo
/// root's config. The global and system configs and a config directly in your
/// home directory, such as ~/mise.toml, are left out unless you pass
/// `--include-global`.
///
/// Build on a Linux host with the image's architecture. Tools are packaged as
/// installed on the host, and so is the running mise binary, at
/// /usr/local/bin/mise, unless you pass `--no-mise`. asdf plugins are not
/// supported; use a vfox plugin or any other backend for each tool. vfox
/// plugins are copied into the image next to the tools they install.
///
/// Requires `mise settings experimental=true` (or `MISE_EXPERIMENTAL=1`).
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r###"mise oci build"###,
        help = r###"Build the project's image into ./mise-oci"###
    ),
    example(
        r###"mise oci build --from ubuntu:24.04 --tag myorg/dev:latest -o ./img"###,
        help = r###"Use another base image and record a tag"###
    ),
    example(r###"skopeo inspect oci:./img"###, help = r###"Inspect the result"###),
    example(
        r###"mise oci run --image-dir ./img -- /bin/sh"###,
        help = r###"Open a shell in it"###
    )
)]
pub(super) struct Build {
    /// Copy a host file, directory, or symlink into the image (repeatable, HOST:IMAGE)
    #[usage(long, value_name = "HOST_PATH:IMAGE_PATH")]
    copy: Vec<OciCopy>,

    /// Output directory for the OCI image layout
    #[usage(long, short, default = "./mise-oci", value_hint = ValueHint::DirPath)]
    output: PathBuf,

    /// Base image reference (overrides [oci].from and the oci.default_from setting)
    #[usage(long)]
    from: Option<String>,

    /// Also package tools from the global and system configs
    ///
    /// By default `mise oci build` packages only project config files, including
    /// a monorepo root's config. With this flag it also includes the tools,
    /// `[oci]`, `[bootstrap.packages]`, and `[dotfiles]` of the global and system
    /// configs and of a config directly in your home directory, so personal tools
    /// from ~/.config/mise/config.toml end up in the image.
    #[usage(long)]
    include_global: bool,

    /// Tag to record in the image index (the org.opencontainers.image.ref.name annotation)
    #[usage(long, short)]
    tag: Option<String>,

    /// Where tools install inside the image
    ///
    /// Overrides [oci].mount_point and the oci.default_mount_point setting.
    #[usage(long)]
    mount_point: Option<String>,

    /// Do not embed the currently-running mise binary at /usr/local/bin/mise
    #[usage(long)]
    no_mise: bool,

    /// Rebuild tool layers without reading or writing the local layer cache
    #[usage(long)]
    no_cache: bool,

    /// UID[:GID] to assign to every tar entry in generated layers
    ///
    /// Overrides [oci].user_id and [oci].group_id. Defaults to 0:0. If GID is
    /// omitted, it defaults to UID. This affects file ownership only; [oci].user
    /// controls the image USER directive.
    #[usage(long, value_name = "UID[:GID]")]
    owner: Option<LayerOwner>,
}

impl Build {
    pub(super) async fn run(self) -> Result<()> {
        Settings::get().ensure_experimental("mise oci build")?;

        let opts = BuildOptions {
            out_dir: self.output.clone(),
            from: self.from.clone(),
            tag: self.tag.clone(),
            mount_point: self.mount_point.clone(),
            owner: self.owner,
            mise_binary: mise_binary_path(self.no_mise),
            copy: self.copy.clone(),
            // Layer reuse would leave blob-less holes in the layout; `build`
            // must produce a complete, standalone image directory.
            reuse_from: None,
            push_destination: None,
            no_cache: self.no_cache,
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
